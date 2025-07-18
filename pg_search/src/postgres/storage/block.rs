// Copyright (c) 2023-2025 ParadeDB, Inc.
//
// This file is part of ParadeDB - Postgres for Search and Analytics
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <http://www.gnu.org/licenses/>.

use crate::postgres::storage::buffer::{Buffer, BufferManager, BufferMut};
use pgrx::*;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Formatter};
use std::hash::Hash;
use std::mem::{offset_of, size_of};
use std::path::{Path, PathBuf};
use std::slice::from_raw_parts;
use tantivy::index::{SegmentComponent, SegmentId};
use tantivy::Opstamp;

// ---------------------------------------------------------
// BM25 page special data
// ---------------------------------------------------------

// Struct for all page's LP_SPECIAL data
#[derive(Clone, Debug)]
pub struct BM25PageSpecialData {
    pub next_blockno: pg_sys::BlockNumber,
    pub xmax: pg_sys::TransactionId,
}

// ---------------------------------------------------------
// Linked lists
// ---------------------------------------------------------

/// Struct held in the first buffer of every linked list's content area
#[derive(Copy, Clone)]
#[repr(C, packed)]
pub struct LinkedListData {
    /// Indicates the first BlockNumber of the linked list.
    pub start_blockno: pg_sys::BlockNumber,

    /// Indicates the last BlockNumber of the linked list.
    pub last_blockno: pg_sys::BlockNumber,

    /// Counts the total number of data pages in the linked list (excludes the header page)
    pub npages: u32,

    /// Indicates where the BlockList for this linked list starts;
    pub blocklist_start: pg_sys::BlockNumber,
}

impl Debug for LinkedListData {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkedListData")
            .field("start_blockno", &{ self.start_blockno })
            .field("last_blockno", &{ self.last_blockno })
            .field("npages", &{ self.npages })
            .field("blocklist_start", &{ self.blocklist_start })
            .finish()
    }
}

/// Every linked list must implement this trait
pub trait LinkedList {
    fn get_header_blockno(&self) -> pg_sys::BlockNumber;

    fn bman(&self) -> &BufferManager;

    fn bman_mut(&mut self) -> &mut BufferManager;

    ///
    /// Get the start blockno of the LinkedList, and return a Buffer for the header block of
    /// the list, which must be held until the start blockno is actually dereferenced.
    ///
    fn get_start_blockno(&self) -> (pg_sys::BlockNumber, Buffer) {
        let buffer = self.bman().get_buffer(self.get_header_blockno());
        let metadata = buffer.page().contents::<LinkedListData>();
        let start_blockno = metadata.start_blockno;
        assert!(start_blockno != 0);
        assert!(start_blockno != pg_sys::InvalidBlockNumber);
        (start_blockno, buffer)
    }

    ///
    /// See `get_start_blockno`.
    ///
    fn get_start_blockno_mut(&mut self) -> (pg_sys::BlockNumber, BufferMut) {
        let header_blockno = self.get_header_blockno();
        let buffer = self.bman_mut().get_buffer_mut(header_blockno);
        let metadata = buffer.page().contents::<LinkedListData>();
        let start_blockno = metadata.start_blockno;
        assert!(start_blockno != 0);
        assert!(start_blockno != pg_sys::InvalidBlockNumber);
        (start_blockno, buffer)
    }

    fn get_last_blockno(&self) -> pg_sys::BlockNumber {
        // TODO: If concurrency is a concern for "append" cases, then we'd want to iterate from the
        // hand-over-hand from the head to the tail rather than jumping immediately to the tail.
        let buffer = self.bman().get_buffer(self.get_header_blockno());
        let metadata = buffer.page().contents::<LinkedListData>();
        let last_blockno = metadata.last_blockno;
        assert!(last_blockno != 0);
        assert!(last_blockno != pg_sys::InvalidBlockNumber);
        last_blockno
    }

    fn block_for_ord(&self, ord: usize) -> Option<pg_sys::BlockNumber>;

    ///
    /// Note: It is not safe to begin iteration of the list using this method, as the buffer for
    /// the metadata is released when it returns. Use `get_start_blockno` to begin iteration.
    fn get_linked_list_data(&self) -> LinkedListData {
        self.bman()
            .get_buffer(self.get_header_blockno())
            .page()
            .contents::<LinkedListData>()
    }
}

// ---------------------------------------------------------
// Linked list entry structs
// ---------------------------------------------------------

/// Metadata for tracking where to find a file
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct FileEntry {
    pub starting_block: pg_sys::BlockNumber,
    pub total_bytes: usize,
}

/// Metadata for tracking where to find a ".del" file
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeleteEntry {
    pub file_entry: FileEntry,
    pub num_deleted_docs: u32,
}

/// Metadata for tracking alive segments
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SegmentMetaEntry {
    pub segment_id: SegmentId,
    pub max_doc: u32,

    /// this is the unused space that was once where we stored the `xmin` transaction id that created this entry
    #[doc(hidden)]
    #[serde(alias = "xmin")]
    pub _unused: pg_sys::TransactionId,

    /// If set to [`pg_sys::FrozenTransactionId`] then this entry has been deleted via a Tantivy merge
    /// and a) is no longer visible to any transaction and b) is subject to being garbage collected
    pub xmax: pg_sys::TransactionId,

    pub postings: Option<FileEntry>,
    pub positions: Option<FileEntry>,
    pub fast_fields: Option<FileEntry>,
    pub field_norms: Option<FileEntry>,
    pub terms: Option<FileEntry>,
    pub store: Option<FileEntry>,
    pub temp_store: Option<FileEntry>,
    pub delete: Option<DeleteEntry>,
}

impl Default for SegmentMetaEntry {
    fn default() -> Self {
        Self {
            segment_id: SegmentId::generate_random(),
            max_doc: Default::default(),
            _unused: pg_sys::InvalidTransactionId,
            xmax: pg_sys::InvalidTransactionId,
            postings: None,
            positions: None,
            fast_fields: None,
            field_norms: None,
            terms: None,
            store: None,
            temp_store: None,
            delete: None,
        }
    }
}

impl SegmentMetaEntry {
    pub fn is_deleted(&self) -> bool {
        self.xmax == pg_sys::FrozenTransactionId
    }

    /// In `save_new_metas`, if a new `DeleteEntry` is created and there was already a `DeleteEntry`,
    /// we create a "fake" `SegmentMetaEntry` that stores the old `DeleteEntry` so it can be garbage collected
    /// later.
    ///
    /// This function returns true if the `SegmentMetaEntry` is a "fake" `DeleteEntry`
    pub fn is_orphaned_delete(&self) -> bool {
        self.segment_id == SegmentId::from_bytes([0; 16])
            && self.xmax == pg_sys::FrozenTransactionId
    }

    /// Fake an `Opstamp` that's always zero
    pub fn opstamp(&self) -> Opstamp {
        0
    }

    pub fn num_docs(&self) -> usize {
        self.max_doc as usize - self.num_deleted_docs()
    }

    pub fn num_deleted_docs(&self) -> usize {
        self.delete
            .map(|entry| entry.num_deleted_docs as usize)
            .unwrap_or(0)
    }

    pub fn file_entry(&self, path: &Path) -> Option<FileEntry> {
        for (file_path, (file_entry, _)) in self.get_component_paths().zip(self.file_entries()) {
            if path == file_path {
                return Some(*file_entry);
            }
        }
        None
    }

    pub fn file_entries(&self) -> impl Iterator<Item = (&FileEntry, SegmentComponent)> {
        self.postings
            .iter()
            .map(|fe| (fe, SegmentComponent::Postings))
            .chain(
                self.positions
                    .iter()
                    .map(|fe| (fe, SegmentComponent::Positions)),
            )
            .chain(
                self.fast_fields
                    .iter()
                    .map(|fe| (fe, SegmentComponent::FastFields)),
            )
            .chain(
                self.field_norms
                    .iter()
                    .map(|fe| (fe, SegmentComponent::FieldNorms)),
            )
            .chain(self.terms.iter().map(|fe| (fe, SegmentComponent::Terms)))
            .chain(
                self.temp_store
                    .iter()
                    .map(|fe| (fe, SegmentComponent::TempStore)),
            )
            .chain(
                self.delete
                    .as_ref()
                    .map(|d| (&d.file_entry, SegmentComponent::Delete)),
            )
    }

    pub fn byte_size(&self) -> u64 {
        let mut size = 0;

        size += self
            .postings
            .as_ref()
            .map(|entry| entry.total_bytes as u64)
            .unwrap_or(0);
        size += self
            .positions
            .as_ref()
            .map(|entry| entry.total_bytes as u64)
            .unwrap_or(0);
        size += self
            .fast_fields
            .as_ref()
            .map(|entry| entry.total_bytes as u64)
            .unwrap_or(0);
        size += self
            .field_norms
            .as_ref()
            .map(|entry| entry.total_bytes as u64)
            .unwrap_or(0);
        size += self
            .terms
            .as_ref()
            .map(|entry| entry.total_bytes as u64)
            .unwrap_or(0);
        size += self
            .store
            .as_ref()
            .map(|entry| entry.total_bytes as u64)
            .unwrap_or(0);
        size += self
            .temp_store
            .as_ref()
            .map(|entry| entry.total_bytes as u64)
            .unwrap_or(0);
        size += self
            .delete
            .as_ref()
            .map(|entry| entry.file_entry.total_bytes as u64)
            .unwrap_or(0);
        size
    }

    pub fn get_component_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        let uuid = self.segment_id.uuid_string();
        self.file_entries().map(move |(_, component)| {
            if matches!(component, SegmentComponent::Delete) {
                PathBuf::from(format!(
                    "{}.0.{}", // we can hardcode zero as the opstamp component of the path as it's not used by anyone
                    uuid,
                    SegmentComponent::Delete
                ))
            } else {
                PathBuf::from(format!("{uuid}.{component}"))
            }
        })
    }
}

// ---------------------------------------------------------
// Linked list entry <-> PgItem
// ---------------------------------------------------------

/// Wrapper for pg_sys::Item that also stores its size
#[derive(Clone)]
pub struct PgItem(pub pg_sys::Item, pub pg_sys::Size);

impl From<SegmentMetaEntry> for PgItem {
    fn from(val: SegmentMetaEntry) -> Self {
        let mut buf = pgrx::StringInfo::new();
        let len = bincode::serde::encode_into_std_write(val, &mut buf, bincode::config::legacy())
            .expect("expected to serialize valid SegmentMetaEntry");
        PgItem(buf.into_char_ptr() as pg_sys::Item, len as pg_sys::Size)
    }
}

impl From<PgItem> for SegmentMetaEntry {
    fn from(pg_item: PgItem) -> Self {
        let PgItem(item, size) = pg_item;
        let (decoded, _) = bincode::serde::decode_from_slice(
            unsafe { from_raw_parts(item as *const u8, size) },
            bincode::config::legacy(),
        )
        .expect("expected to deserialize valid SegmentMetaEntry");
        decoded
    }
}

pub trait SegmentFileDetails {
    fn segment_id(&self) -> Option<SegmentId>;
    fn component_type(&self) -> Option<SegmentComponent>;
}

impl<T: AsRef<Path>> SegmentFileDetails for T {
    fn segment_id(&self) -> Option<SegmentId> {
        let mut parts = self.as_ref().file_name()?.to_str()?.split('.');
        SegmentId::from_uuid_string(parts.next()?).ok()
    }

    fn component_type(&self) -> Option<SegmentComponent> {
        let mut parts = self.as_ref().file_name()?.to_str()?.split('.');
        let _ = parts.next()?; // skip segment id
        let mut extension = parts.next()?;
        if let Some(last) = parts.next() {
            // it has three parts, so the extension is instead the last part
            extension = last;
        }
        SegmentComponent::try_from(extension).ok()
    }
}

// ---------------------------------------------------------
// Linked list entry MVCC methods
// ---------------------------------------------------------

pub trait MVCCEntry {
    fn pintest_blockno(&self) -> pg_sys::BlockNumber;

    // Provided methods
    unsafe fn visible(&self) -> bool;

    unsafe fn recyclable(&self, bman: &mut BufferManager) -> bool;

    unsafe fn mergeable(&self) -> bool;
}

impl MVCCEntry for SegmentMetaEntry {
    fn pintest_blockno(&self) -> pg_sys::BlockNumber {
        match self.file_entries().next() {
            None => panic!("SegmentMetaEntry for `{}` has no files", self.segment_id),
            Some((file_entry, _)) => file_entry.starting_block,
        }
    }

    unsafe fn visible(&self) -> bool {
        // visible if we haven't deleted it
        !self.is_deleted()
    }

    unsafe fn recyclable(&self, bman: &mut BufferManager) -> bool {
        // recyclable if we've deleted it
        self.is_deleted()

        // and there's no pin on our pintest buffer, assuming we have a valid buffer
        && (self.pintest_blockno() == pg_sys::InvalidBlockNumber || bman.get_buffer_for_cleanup_conditional(self.pintest_blockno()).is_some())
    }

    unsafe fn mergeable(&self) -> bool {
        // mergeable if we haven't deleted it
        !self.is_deleted()
    }
}

pub const fn bm25_max_free_space() -> usize {
    unsafe {
        (pg_sys::BLCKSZ as usize)
            - pg_sys::MAXALIGN(size_of::<BM25PageSpecialData>())
            - pg_sys::MAXALIGN(offset_of!(pg_sys::PageHeaderData, pd_linp))
    }
}

#[inline(always)]
pub fn block_number_is_valid(block_number: pg_sys::BlockNumber) -> bool {
    block_number != 0 && block_number != pg_sys::InvalidBlockNumber
}
