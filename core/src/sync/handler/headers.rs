/*******************************************************************************
 * Copyright (c) 2018-2019 Aion foundation.
 *
 *     This file is part of the aion network project.
 *
 *     The aion network project is free software: you can redistribute it
 *     and/or modify it under the terms of the GNU General Public License
 *     as published by the Free Software Foundation, either version 3 of
 *     the License, or any later version.
 *
 *     The aion network project is distributed in the hope that it will
 *     be useful, but WITHOUT ANY WARRANTY; without even the implied
 *     warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
 *     See the GNU General Public License for more details.
 *
 *     You should have received a copy of the GNU General Public License
 *     along with the aion network project source files.
 *     If not, see <https://www.gnu.org/licenses/>.
 *
 ******************************************************************************/

use std::mem;
use std::time::{Duration, SystemTime};
use std::sync::{RwLock,Arc};
use std::collections::{HashMap,BTreeMap};
use client::BlockId;
use engine::unity_engine::UnityEngine;
use header::{Header as BlockHeader,Seal};
use acore_bytes::to_hex;
use byteorder::{BigEndian, ByteOrder, ReadBytesExt};
use bytes::BufMut;
use rlp::{RlpStream, UntrustedRlp};
use p2p::ChannelBuffer;
use p2p::Node;
use p2p::get_random_active_node;
use p2p::send as p2p_send;
use sync::route::VERSION;
use sync::route::MODULE;
use sync::route::ACTION;
use sync::event::SyncEvent;
use sync::storage::{ SyncStorage};
use sync::helper::{Wrapper,WithStatus};

const BACKWARD_SYNC_STEP: u64 = 64;
pub const REQUEST_SIZE: u32 = 64;
const LARGE_REQUEST_SIZE: u64 = 48;

pub fn get_headers_from_node(node: &mut Node) {
    trace!(target: "sync", "get_headers_from_node, node id: {}", node.get_id_string());
    //    if node.target_total_difficulty > node.current_total_difficulty {
    //        let mut from: u64 = 1;
    //        let mut size = REQUEST_SIZE;
    //
    //        match node.mode {
    //            Mode::LIGHTNING => {
    //                // request far forward blocks
    //                let mut self_num;
    //                let max_staged_block_number = SyncStorage::get_max_staged_block_number();
    //                let synced_block_number = SyncStorage::get_synced_block_number();
    //                if synced_block_number + LARGE_REQUEST_SIZE * 5 > max_staged_block_number {
    //                    let sync_speed = SyncStorage::get_sync_speed();
    //                    let jump_size = if sync_speed <= 40 {
    //                        480
    //                    } else if sync_speed > 40 && sync_speed <= 100 {
    //                        sync_speed as u64 * 12
    //                    } else {
    //                        1200
    //                    };
    //                    self_num = synced_block_number + jump_size;
    //                } else {
    //                    self_num = max_staged_block_number + 1;
    //                }
    //                if node.best_block_num > self_num + LARGE_REQUEST_SIZE {
    //                    size = LARGE_REQUEST_SIZE;
    //                    from = self_num;
    //                } else {
    //                    // transition to ramp down strategy
    //                    node.mode = Mode::THUNDER;
    //                    return;
    //                }
    //            }
    //            Mode::THUNDER => {
    //                let mut self_num = SyncStorage::get_synced_block_number();
    //                size = LARGE_REQUEST_SIZE;
    //                from = if self_num > 4 { self_num - 3 } else { 1 };
    //            }
    //            Mode::NORMAL => {
    //                let self_num = SyncStorage::get_synced_block_number();
    //                let node_num = node.best_block_num;
    //
    //                if node_num >= self_num + BACKWARD_SYNC_STEP {
    //                    from = if self_num > 4 { self_num - 3 } else { 1 };
    //                } else if self_num < BACKWARD_SYNC_STEP {
    //                    from = if self_num > 16 { self_num - 15 } else { 1 };
    //                } else if node_num >= self_num - BACKWARD_SYNC_STEP {
    //                    from = self_num - 16;
    //                } else {
    //                    return;
    //                    // return;
    //                    // ------
    //                    // FIX: must consider a case when syncing backward (chain reorg) node_num < self_num - BACKWARD_SYNC_STEP
    //                    // ------
    //                    // from = node_num;
    //                }
    //            }
    //            Mode::BACKWARD => {
    //                let self_num = node.synced_block_num;
    //                if self_num > BACKWARD_SYNC_STEP {
    //                    from = self_num - BACKWARD_SYNC_STEP;
    //                }
    //            }
    //            Mode::FORWARD => {
    //                let self_num = node.synced_block_num;
    //                from = self_num + 1;
    //            }
    //        };
    //
    //        if node.last_request_num != from {
    //            node.last_request_timestamp = SystemTime::now();
    //        }
    //        node.last_request_num = from;
    //
    //        debug!(target: "sync", "request headers: from number: {}, node: {}, sn: {}, mode: {}.", from, node.get_ip_addr(), node.synced_block_num, node.mode);
    //
    //        send_blocks_headers_req(node.node_hash, from, size as u32);
    //        update_node(node.node_hash, node);
    //    }
}

pub fn send(start: u64, hash: u64, nodes: Arc<RwLock<HashMap<u64, Node>>>) {
    let mut cb = ChannelBuffer::new();
    cb.head.ver = VERSION::V0.value();
    cb.head.ctrl = MODULE::SYNC.value();
    cb.head.action = ACTION::HEADERSREQ.value();

    let mut from_buf = [0u8; 8];
    BigEndian::write_u64(&mut from_buf, start);
    cb.body.put_slice(&from_buf);

    let mut size_buf = [0u8; 4];
    BigEndian::write_u32(&mut size_buf, REQUEST_SIZE);
    cb.body.put_slice(&size_buf);

    cb.head.len = cb.body.len() as u32;
    p2p_send(&hash, cb, nodes.clone());
}

pub fn receive_req(hash: u64, cb_in: ChannelBuffer, nodes: Arc<RwLock<HashMap<u64, Node>>>) {
    trace!(target: "sync", "headers/receive_req");

    let client = SyncStorage::get_block_chain();

    let mut res = ChannelBuffer::new();

    res.head.ver = VERSION::V0.value();
    res.head.ctrl = MODULE::SYNC.value();
    res.head.action = ACTION::HEADERSRES.value();

    let mut res_body = Vec::new();

    let (mut from, req_body_rest) = cb_in.body.split_at(mem::size_of::<u64>());
    let from = from.read_u64::<BigEndian>().unwrap_or(1);
    let (mut size, _) = req_body_rest.split_at(mem::size_of::<u32>());
    let size = size.read_u32::<BigEndian>().unwrap_or(1);
    let chain_info = client.chain_info();
    let last = chain_info.best_block_number;

    let mut header_count = 0;
    let number = from;
    let mut data = Vec::new();
    while number + header_count <= last && header_count < size.into() {
        match client.block_header(BlockId::Number(number + header_count)) {
            Some(hdr) => {
                data.append(&mut hdr.into_inner());
                header_count += 1;
            }
            None => {}
        }
    }

    if header_count > 0 {
        let mut rlp = RlpStream::new_list(header_count as usize);

        rlp.append_raw(&data, header_count as usize);
        res_body.put_slice(rlp.as_raw());
    }

    res.body.put_slice(res_body.as_slice());
    res.head.len = res.body.len() as u32;

    //    SyncEvent::update_node_state(node, SyncEvent::OnBlockHeadersReq);
    //    update_node(node_hash, node);
    p2p_send(&hash, res, nodes);
}

pub fn receive_res(
    hash: u64,
    cb_in: ChannelBuffer,
    _nodes: Arc<RwLock<HashMap<u64, Node>>>,
    hws: Arc<RwLock<BTreeMap<u64, Wrapper>>>,
)
{
    trace!(target: "sync", "headers/receive_res");

    let rlp = UntrustedRlp::new(cb_in.body.as_slice());
    let mut prev_header = BlockHeader::new();
    let mut hw = Wrapper::new();
    let mut headers = Vec::new();
    let mut number = 0;
    for header_rlp in rlp.iter() {
        if let Ok(header) = header_rlp.as_val() {
            let result = UnityEngine::validate_block_header(&header);
            match result {
                Ok(()) => {
                    // break if not consisting
                    if prev_header.number() != 0
                        && (header.number() != prev_header.number() + 1
                            || prev_header.hash() != *header.parent_hash())
                    {
                        error!(target: "sync",
                            "<inconsistent-block-headers num={}, prev+1={}, hash={}, p_hash={}>, hash={}>",
                            header.number(),
                            prev_header.number() + 1,
                            header.parent_hash(),
                            prev_header.hash(),
                            header.hash(),
                        );
                        break;
                    } else {
                        //                        let hash = header.hash();
                        //                        let number = header.number();

                        // Skip staged block header
                        //                        if node.mode == Mode::THUNDER {
                        //                            if SyncStorage::is_staged_block_hash(hash) {
                        //                                debug!(target: "sync", "Skip staged block header #{}: {:?}", number, hash);
                        //                                // hw.headers.push(header.clone());
                        //                                break;
                        //                            }
                        //                        }

                        //                        if !SyncStorage::is_downloaded_block_hashes(&hash)
                        //                            && !SyncStorage::is_imported_block_hash(&hash)
                        //                        {
                        number = header.number();
                        headers.push(header.clone().rlp(Seal::Without));
                        //                        }
                    }
                    prev_header = header;
                }
                Err(e) => {
                    // ignore this batch if any invalidated header
                    error!(target: "sync", "Invalid header: {:?}, header: {}", e, to_hex(header_rlp.as_raw()));
                }
            }
        } else {
            error!(target: "sync", "Invalid header: {}", to_hex(header_rlp.as_raw()));
        }
    }

    if !headers.is_empty() {
        hw.node_hash = hash;
        hw.with_status = WithStatus::GetHeader(headers);
        hw.timestamp = SystemTime::now();
        if let Ok(mut hws) = hws.try_write() {
            info!(target: "sync", "get headers to #{}", number);
            hws.insert(number, hw);
        }
    } else {
        debug!(target: "sync", "Came too late............");
    }

    //    SyncEvent::update_node_state(node, SyncEvent::OnBlockHeadersRes);
    //    update_node(node_hash, node);
}
