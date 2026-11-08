#![cfg(windows)]

use anyhow::{Context, Result, anyhow};
use deku::prelude::*;
use std::ffi::CString;
use std::slice;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Memory::{
    FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingA, UnmapViewOfFile,
};
use windows::core::PCSTR;

const IRSDK_MEMMAPFILENAME: &str = "Local\\IRSDKMemMapFileName";
const IRSDK_MAX_BUFS: usize = 4;
const IRSDK_MAX_STRING: usize = 32;

// iRacing SDK header (irsdk_header).
#[derive(Debug, Clone, DekuRead)]
#[deku(endian = "little")]
struct Header {
    _version: i32,
    status: i32,
    _tick_rate: i32,
    session_info_update: i32,
    session_info_len: i32,
    session_info_offset: i32,
    num_vars: i32,
    var_header_offset: i32,
    num_buf: i32,
    buf_len: i32,
    #[deku(count = "2")]
    _pad1: Vec<i32>,
    #[deku(count = "IRSDK_MAX_BUFS")]
    var_buf: Vec<VarBuf>,
}

#[derive(Debug, Clone, DekuRead)]
#[deku(ctx = "endian: deku::ctx::Endian", endian = "endian")]
struct VarBuf {
    tick_count: i32,
    buf_offset: i32,
    #[deku(count = "2")]
    _pad: Vec<i32>,
}

#[derive(Debug, Clone, DekuRead)]
#[deku(endian = "little")]
struct VarHeader {
    var_type: i32,      // offset 0, 4 bytes
    offset: i32,        // offset 4, 4 bytes
    count: i32,         // offset 8, 4 bytes
    _count_as_time: u8, // offset 12, 1 byte (bool in C)
    #[deku(count = "3")]
    _pad: Vec<u8>, // offset 13, 3 bytes padding
    #[deku(count = "IRSDK_MAX_STRING")]
    name: Vec<u8>, // offset 16, 32 bytes
    #[deku(count = "64")]
    _desc: Vec<u8>, // offset 48, 64 bytes
    #[deku(count = "IRSDK_MAX_STRING")]
    _unit: Vec<u8>, // offset 112, 32 bytes
}

// Variable types (irsdk_VarType).
#[allow(dead_code)]
const IRSDK_CHAR: i32 = 0;
#[allow(dead_code)]
const IRSDK_BOOL: i32 = 1;
const IRSDK_INT: i32 = 2;
#[allow(dead_code)]
const IRSDK_BITFIELD: i32 = 3;
const IRSDK_FLOAT: i32 = 4;
const IRSDK_DOUBLE: i32 = 5;

pub struct IrSdkClient {
    file_mapping: HANDLE,
    shared_mem: MEMORY_MAPPED_VIEW_ADDRESS,
}

impl IrSdkClient {
    pub fn new() -> Result<Self> {
        unsafe {
            let name = CString::new(IRSDK_MEMMAPFILENAME)?;
            let file_mapping = OpenFileMappingA(
                FILE_MAP_READ.0,
                false,
                PCSTR::from_raw(name.as_ptr() as *const u8),
            )
            .context("Cannot open the iRacing shared memory. Make sure that iRacing runs.")?;

            let shared_mem = MapViewOfFile(file_mapping, FILE_MAP_READ, 0, 0, 0);
            if shared_mem.Value.is_null() {
                CloseHandle(file_mapping).ok();
                return Err(anyhow!("Cannot map the iRacing shared memory"));
            }

            Ok(Self {
                file_mapping,
                shared_mem,
            })
        }
    }

    pub fn is_connected(&self) -> bool {
        unsafe {
            let header = self.read_header();
            header.status == 1
        }
    }

    /// Returns the session info update counter. iRacing increments it when the
    /// session info changes.
    pub fn get_session_info_update(&self) -> i32 {
        unsafe {
            let header = self.read_header();
            header.session_info_update
        }
    }

    pub fn get_session_info(&self) -> Result<String> {
        unsafe {
            let header = self.read_header();

            let session_info_offset = header.session_info_offset as usize;
            let session_info_len = header.session_info_len as usize;

            if session_info_len == 0 {
                return Err(anyhow!("No session info available"));
            }

            let data_ptr = (self.shared_mem.Value as *const u8).add(session_info_offset);
            let data = slice::from_raw_parts(data_ptr, session_info_len);

            // iRacing sometimes writes UTF-8 that is not valid, so use from_utf8_lossy.
            let yaml = String::from_utf8_lossy(data)
                .trim_end_matches('\0')
                .to_string();

            Ok(yaml)
        }
    }

    unsafe fn read_header(&self) -> Header {
        // The header is 112 bytes: 10 i32s, 2 padding i32s and 4 VarBufs of 4 i32s.
        // This reads 4 more bytes (11 i32s). Deku reads only the first 112.
        let header_size = 11 * 4 + 2 * 4 + IRSDK_MAX_BUFS * 4 * 4;
        // SAFETY: The caller makes sure that the shared memory is mapped. The mapping is larger
        // than the header.
        let data =
            unsafe { slice::from_raw_parts(self.shared_mem.Value as *const u8, header_size) };
        let (_, header) = Header::from_bytes((data, 0)).expect("cannot parse the iRacing header");
        header
    }

    pub fn get_latest_data(&self) -> Result<Vec<u8>> {
        unsafe {
            let header = self.read_header();

            if header.status != 1 {
                return Err(anyhow!("iRacing not connected"));
            }

            // Read the second most recent buffer. iRacing can still write to the most
            // recent buffer, so its data can be incomplete.
            let mut buffers: Vec<(usize, i32)> = (0..header.num_buf as usize)
                .map(|i| (i, header.var_buf[i].tick_count))
                .collect();

            buffers.sort_by_key(|&(_, tick)| std::cmp::Reverse(tick));

            let latest_buf_idx = if buffers.len() > 1 {
                buffers[1].0
            } else {
                buffers[0].0
            };

            let buf = &header.var_buf[latest_buf_idx];
            let buf_offset = buf.buf_offset as usize;
            let buf_len = header.buf_len as usize;

            let data_ptr = (self.shared_mem.Value as *const u8).add(buf_offset);
            let data = slice::from_raw_parts(data_ptr, buf_len);

            Ok(data.to_vec())
        }
    }

    fn find_var(&self, name: &str) -> Result<VarHeader> {
        unsafe {
            let header = self.read_header();
            let var_header_offset = header.var_header_offset as usize;
            let num_vars = header.num_vars as usize;

            // 4 + 4 + 4 + 1 + 3 + 32 + 64 + 32 bytes. See VarHeader.
            const VAR_HEADER_SIZE: usize = 144;

            for i in 0..num_vars {
                let var_offset = var_header_offset + (i * VAR_HEADER_SIZE);
                let var_data = slice::from_raw_parts(
                    (self.shared_mem.Value as *const u8).add(var_offset),
                    VAR_HEADER_SIZE,
                );

                let (_, var) = VarHeader::from_bytes((var_data, 0))
                    .map_err(|e| anyhow!("Cannot parse VarHeader: {}", e))?;

                let var_name = std::str::from_utf8(&var.name)
                    .unwrap_or("")
                    .trim_end_matches('\0');

                if var_name == name {
                    return Ok(var);
                }
            }

            Err(anyhow!("Variable '{}' not found", name))
        }
    }

    pub fn get_var_f32(&self, name: &str, data: &[u8]) -> Result<f32> {
        let var = self.find_var(name)?;

        if var.var_type != IRSDK_FLOAT {
            return Err(anyhow!(
                "Variable '{}' is not a float (type={})",
                name,
                var.var_type
            ));
        }

        let offset = var.offset as usize;
        if offset + 4 > data.len() {
            return Err(anyhow!("Data buffer too small for variable '{}'", name));
        }

        unsafe {
            let ptr = data.as_ptr().add(offset) as *const f32;
            Ok(ptr.read_unaligned())
        }
    }

    pub fn get_var_f64(&self, name: &str, data: &[u8]) -> Result<f64> {
        let var = self.find_var(name)?;

        if var.var_type != IRSDK_DOUBLE {
            return Err(anyhow!(
                "Variable '{}' is not a double (type={})",
                name,
                var.var_type
            ));
        }

        let offset = var.offset as usize;
        if offset + 8 > data.len() {
            return Err(anyhow!("Data buffer too small for variable '{}'", name));
        }

        unsafe {
            let ptr = data.as_ptr().add(offset) as *const f64;
            Ok(ptr.read_unaligned())
        }
    }

    pub fn get_var_i32(&self, name: &str, data: &[u8]) -> Result<i32> {
        let var = self.find_var(name)?;

        if var.var_type != IRSDK_INT && var.var_type != IRSDK_BOOL {
            return Err(anyhow!(
                "Variable '{}' is not an int (type={})",
                name,
                var.var_type
            ));
        }

        let offset = var.offset as usize;
        if offset + 4 > data.len() {
            return Err(anyhow!("Data buffer too small for variable '{}'", name));
        }

        unsafe {
            let ptr = data.as_ptr().add(offset) as *const i32;
            Ok(ptr.read_unaligned())
        }
    }

    pub fn get_var_u32(&self, name: &str, data: &[u8]) -> Result<u32> {
        let var = self.find_var(name)?;

        let offset = var.offset as usize;
        if offset + 4 > data.len() {
            return Err(anyhow!("Data buffer too small for variable '{}'", name));
        }

        unsafe {
            let ptr = data.as_ptr().add(offset) as *const u32;
            Ok(ptr.read_unaligned())
        }
    }

    pub fn get_var_f32_array(&self, name: &str, data: &[u8]) -> Result<Vec<f32>> {
        let var = self.find_var(name)?;

        if var.var_type != IRSDK_FLOAT {
            return Err(anyhow!(
                "Variable '{}' is not a float array (type={})",
                name,
                var.var_type
            ));
        }

        let offset = var.offset as usize;
        let count = var.count as usize;
        let size = count * 4;

        if offset + size > data.len() {
            return Err(anyhow!("Data buffer too small for variable '{}'", name));
        }

        let mut result = Vec::with_capacity(count);
        unsafe {
            for i in 0..count {
                let ptr = data.as_ptr().add(offset + i * 4) as *const f32;
                result.push(ptr.read_unaligned());
            }
        }

        Ok(result)
    }

    pub fn get_var_i32_array(&self, name: &str, data: &[u8]) -> Result<Vec<i32>> {
        let var = self.find_var(name)?;

        if var.var_type != IRSDK_INT {
            return Err(anyhow!(
                "Variable '{}' is not an int array (type={})",
                name,
                var.var_type
            ));
        }

        let offset = var.offset as usize;
        let count = var.count as usize;
        let size = count * 4;

        if offset + size > data.len() {
            return Err(anyhow!("Data buffer too small for variable '{}'", name));
        }

        let mut result = Vec::with_capacity(count);
        unsafe {
            for i in 0..count {
                let ptr = data.as_ptr().add(offset + i * 4) as *const i32;
                result.push(ptr.read_unaligned());
            }
        }

        Ok(result)
    }
}

impl Drop for IrSdkClient {
    fn drop(&mut self) {
        unsafe {
            if !self.shared_mem.Value.is_null() {
                UnmapViewOfFile(self.shared_mem).ok();
            }
            if !self.file_mapping.is_invalid() {
                CloseHandle(self.file_mapping).ok();
            }
        }
    }
}

unsafe impl Send for IrSdkClient {}
unsafe impl Sync for IrSdkClient {}
