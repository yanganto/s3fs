extern crate colored;
extern crate ctrlc;
extern crate crossbeam_channel;
extern crate fuse;
extern crate libc;
extern crate time;
extern crate toml;
#[macro_use]
extern crate serde_derive;
extern crate s3handler;
#[macro_use]
extern crate log;

use std::path::Path;
use std::ffi::OsStr;
use std::time::Duration;
use std::fs::{File, OpenOptions};
use std::io;
use std::io::{Read, Write, BufReader, BufRead};

use fuse::{FileAttr, Filesystem, Request, ReplyAttr, ReplyEntry, ReplyDirectory, FileType, 
    ReplyData};
use colored::*;
use libc::ENOENT;
use time::Timespec;
use crossbeam_channel::{bounded, tick, Receiver, select};
use users::get_current_uid;
use log::{Record, Level, Metadata, LevelFilter};

static MY_LOGGER: MyLogger = MyLogger;

struct MyLogger;

impl log::Log for MyLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Trace
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            match record.level() {
                log::Level::Error => println!("{} - {}", "ERROR".red().bold(), record.args()),
                log::Level::Warn => println!("{} - {}", "WARN".red(), record.args()),
                log::Level::Info => println!("{} - {}", "INFO".cyan(), record.args()),
                log::Level::Debug => println!("{} - {}", "DEBUG".blue().bold(), record.args()),
                log::Level::Trace => println!("{} - {}", "TRACE".blue(), record.args())
            }
            
        }
    }
    fn flush(&self) {}
}

const TTL: Timespec = Timespec { sec: 1, nsec: 0 };                     // 1 second

const CREATE_TIME: Timespec = Timespec { sec: 1381237736, nsec: 0 };    // 2013-10-08 08:56


const HELLO_TXT_CONTENT: &'static str = "Hello World!\n";


#[derive(Debug, Deserialize)]
struct MountConfig {
    bucket: String,
    path:String
}

#[derive(Debug, Deserialize)]
struct Config {
    auth: s3handler::CredentialConfig,
    mount: Vec<MountConfig>
}

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

fn ctrl_channel() -> Result<Receiver<()>, ctrlc::Error> {
    let (sender, receiver) = bounded(100);
    ctrlc::set_handler(move || {
        let _ = sender.send(());
    })?;

    Ok(receiver)
}

fn main() {
    let mut s3fscfg = std::env::home_dir().unwrap();
    s3fscfg.push(".s3fs.toml");

    let mut f;
    if s3fscfg.exists() {
        f = File::open(s3fscfg).expect("s3fs config file not found");
    } else {
        f = File::create(s3fscfg).expect("Can not write s3fs config file");
        let _ = f.write_all(
            b"[auth]\ns3_type = \"aws\"\nhost = \"s3.us-east-1.amazonaws.com\"\nuser = \"admin\"\naccess_key = \"L2D11MY86GEVA6I4DX2S\"\nsecrete_key = \"MBCqT90XMUaBcWd1mcUjPPLdFuNZndiBk0amnVVg\"\nregion = \"us-east-1\"\n[[mount]]\nbucket = \"bucket name\"\npath = \"/mnt\""
            );
        print!("Config file {} is created in your home folder, please edit it and add your credentials", ".s3fs.toml".bold());
        return 
    }
    let mut config_contents = String::new();
    f.read_to_string(&mut config_contents).expect("s3fs config is not readable");

    let config:Config = toml::from_str(config_contents.as_str()).unwrap();

    let mount_point_list = config.mount;

    let ctrl_c_events = ctrl_channel().unwrap();
    let ticks = tick(Duration::from_secs(1));

    let mut _session = Vec::new();
    {
        unsafe {
            for path in mount_point_list.into_iter().map(|m| m.path.clone()) {
                _session.push(
                    fuse::spawn_mount(
                        S3Filesystem{current_uid:get_current_uid()}, 
                        &path, &[]).unwrap()
                );
            }
        }
        loop {
            select! {
                recv(ticks) -> _ => {}
                recv(ctrl_c_events) -> _ => {
                    println!("umount s3fs");
                    break;
                }
            }
        }
    }
}
