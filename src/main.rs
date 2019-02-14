extern crate fuse;
extern crate colored;
extern crate libc;
extern crate time;
extern crate ctrlc;
extern crate crossbeam_channel;

use std::path::Path;
use std::ffi::OsStr;
use std::time::Duration;

use fuse::{FileAttr, Filesystem, Request, ReplyAttr, ReplyEntry, ReplyDirectory, FileType, 
    ReplyData};
use colored::*;
use libc::ENOENT;
use time::Timespec;
use crossbeam_channel::{bounded, tick, Receiver, select};
use users::get_current_uid;

const TTL: Timespec = Timespec { sec: 1, nsec: 0 };                 // 1 second

const CREATE_TIME: Timespec = Timespec { sec: 1381237736, nsec: 0 };    // 2013-10-08 08:56


const HELLO_TXT_CONTENT: &'static str = "Hello World!\n";


struct S3Filesystem {
    current_uid: u32,
}

impl Filesystem for S3Filesystem {
    fn lookup (&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let HELLO_TXT_ATTR: FileAttr = FileAttr {
            ino: 2,
            size: 13,
            blocks: 1,
            atime: CREATE_TIME,
            mtime: CREATE_TIME,
            ctime: CREATE_TIME,
            crtime: CREATE_TIME,
            kind: FileType::RegularFile,
            perm: 511u16,
            nlink: 1,
            uid: self.current_uid,
            gid: self.current_uid,
            rdev: 0,
            flags: 0,
        };
        if parent == 1 && name.to_str() == Some("hello.txt") {
            reply.entry(&TTL, &HELLO_TXT_ATTR, 0);
        } else {
            reply.error(ENOENT);
        }
    }

    fn getattr (&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        let HELLO_DIR_ATTR: FileAttr = FileAttr {
            ino: 1,
            size: 0,
            blocks: 0,
            atime: CREATE_TIME,
            mtime: CREATE_TIME,
            ctime: CREATE_TIME,
            crtime: CREATE_TIME,
            kind: FileType::Directory,
            perm: 448u16, 
            nlink: 2,
            uid: self.current_uid,
            gid: self.current_uid,
            rdev: 0,
            flags: 0,
        };
        let HELLO_TXT_ATTR: FileAttr = FileAttr {
            ino: 2,
            size: 13,
            blocks: 1,
            atime: CREATE_TIME,
            mtime: CREATE_TIME,
            ctime: CREATE_TIME,
            crtime: CREATE_TIME,
            kind: FileType::RegularFile,
            perm: 511u16,
            nlink: 1,
            uid: self.current_uid,
            gid: self.current_uid,
            rdev: 0,
            flags: 0,
        };
        match ino {
            1 => reply.attr(&TTL, &HELLO_DIR_ATTR),
            2 => reply.attr(&TTL, &HELLO_TXT_ATTR),
            _ => reply.error(ENOENT),
        }
    }

    fn read (&mut self, _req: &Request, ino: u64, _fh: u64, _offset: i64, _size: u32, reply: ReplyData) {
        if ino == 2 {
            reply.data(HELLO_TXT_CONTENT.as_bytes());
        } else {
            reply.error(ENOENT);
        }
    }

    fn readdir (&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, 
                mut reply: ReplyDirectory) {
        if ino == 1 {
            if offset == 0 {
                reply.add(1, 0, FileType::Directory, &Path::new("."));
                reply.add(1, 1, FileType::Directory, &Path::new(".."));
                reply.add(2, 2, FileType::RegularFile, &Path::new("hello.txt"));
            }
            reply.ok();
        } else {
            reply.error(ENOENT);
        }
    }
}

fn usage() {
    println!(
        r#"Usage: 

    {} {}"#, 
    std::env::args().nth(0).unwrap_or("s3fs".to_string()).bold(), 
    "<MOUNTPOINT>".blue());
}

fn ctrl_channel() -> Result<Receiver<()>, ctrlc::Error> {
    let (sender, receiver) = bounded(100);
    ctrlc::set_handler(move || {
        let _ = sender.send(());
    })?;

    Ok(receiver)
}

fn main() {
    let mountpoint = match std::env::args().nth(1) {
        Some(p) => { p },
        None => {
            usage();
            return;
        }
    };

    let ctrl_c_events = ctrl_channel().unwrap();
    let ticks = tick(Duration::from_secs(1));

    let _session;
    {
        unsafe {
            _session = fuse::spawn_mount(S3Filesystem{current_uid:get_current_uid()}, &mountpoint, &[]).unwrap();
        }
        loop {
            select! {
                recv(ticks) -> _ => {}
                recv(ctrl_c_events) -> _ => {
                    println!("umount {}", mountpoint);
                    break;
                }
            }
        }
    }
}
