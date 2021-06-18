#![feature(seek_stream_len)]

#[macro_use]
extern crate log;
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufRead, Seek};
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime};

fn mount() {
    let output = Command::new("curl")
        .args(&[
            "-v", "-H", "Content-Type: application/json",
            "admin:admin@localhost:8181/restconf/config/network-topology:network-topology/topology/cli/node/ME_CLI",
            "-X", "PUT", "-d",
            r#"{
                "network-topology:node" :
                {
                  "network-topology:node-id" : "ME_CLI",
                  "cli-topology:host" : "192.168.1.223",
                  "cli-topology:port" : "23",
                  "cli-topology:transport-type" : "telnet",
                  "cli-topology:device-type" : "ios xr",
                  "cli-topology:device-version" : "6.6.1",
                  "cli-topology:username" : "cisco",
                  "cli-topology:password" : "ciscocisco",
                  "node-extension:reconcile": false,
                  "cli-topology:journal-size": 150,
                  "cli-topology:dry-run-journal-size": 150,
                  "cli-topology:keepalive-timeout" : 180
                }
            }"#
        ])
        .output();
    let output = output.unwrap();
    assert!(
        output.status.success(),
        "Operation mount failed. {:?}",
        output
    );
    info!("mount ok");
}

fn unmount(tail: &mut Tail) -> HashSet<LogResult> {
    let output = Command::new("curl")
        .args(&[
            "-v", "-H", "Content-Type: application/json",
            "admin:admin@localhost:8181/restconf/config/network-topology:network-topology/topology/cli/node/ME_CLI",
            "-X", "DELETE"
        ])
        .output();
    let output = output.unwrap();
    assert!(
        output.status.success(),
        "Operation mount failed. {:?}",
        output
    );
    get_last_log(tail)
}

#[derive(Debug, PartialEq, Eq, Hash)]
enum LogResult {
    PromptResolved,
    DeviceSuccessfullyMounted,
    OnDeviceDisconnected,
}

fn get_last_log(tail: &mut Tail) -> HashSet<LogResult> {
    let lines = tail.read_lines().unwrap();
    let mut result = HashSet::new();
    for maybe_line in lines {
        if let Ok(line) = maybe_line {
            if line.contains("Mount point already exists") {
                panic!("Found the error state");
            } else if line.contains("Device successfully mounted") {
                result.insert(LogResult::DeviceSuccessfullyMounted);
            } else if line.contains("Prompt resolved") {
                result.insert(LogResult::PromptResolved);
            } else if line.contains("Device state updated successfully: onDeviceDisconnected") {
                result.insert(LogResult::OnDeviceDisconnected);
            };
        }
    }
    if !result.is_empty() {
        trace!("get_last_log - {:?}", result);
    }
    result
}

fn wait_for_prompt(tail: &mut Tail) -> bool {
    debug!("wait_for_prompt");
    let now = SystemTime::now();
    let max_duration = Duration::from_secs(15);
    while now.elapsed().unwrap() < max_duration {
        if get_last_log(tail).contains(&LogResult::PromptResolved) {
            info!("wait_for_prompt - ok");
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    warn!("wait_for_prompt - nok");
    false
}

fn healthcheck(tail: &mut Tail) -> bool {
    debug!("healthcheck");
    if wait_for_prompt(tail) {
        let now = SystemTime::now();
        let max_duration = Duration::from_secs(15);
        while now.elapsed().unwrap() < max_duration {
            if get_last_log(tail).contains(&LogResult::DeviceSuccessfullyMounted) {
                info!("healthcheck ok");
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    warn!("healthcheck - nok");
    false
}

fn wait_for_unmount(tail: &mut Tail) -> bool {
    debug!("wait_for_unmount");
    let now = SystemTime::now();
    let max_duration = Duration::from_secs(10);
    while now.elapsed().unwrap() < max_duration  {
        if unmount(tail).contains(&LogResult::OnDeviceDisconnected) {
            info!("wait_for_unmount - ok");
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    warn!("wait_for_unmount - nok");
    false
}

#[derive(Debug)]
struct Tail {
    file: File,
    position: u64,
    last_length: u64,
}

impl Tail {
    fn new<P: AsRef<Path>>(filename: P) -> io::Result<Tail> {
        let mut file = File::open(filename)?;
        let len = file.stream_len()?;
        Ok(Tail {
            file,
            position: 0,
            last_length: len,
        })
    }

    fn read_lines(&mut self) -> io::Result<io::Lines<io::BufReader<File>>> {
        // best effort check if the file was rolled back
        let new_length = self.file.stream_len()?;
        if new_length < self.last_length {
            warn!("Detected logfile rollback, new length: {}, old length:{}", new_length, self.last_length);
            self.last_length = new_length;
            self.file.seek(io::SeekFrom::Start(0))?;
        }
        let buf_reader = io::BufReader::new(self.file.try_clone()?);
        Ok(buf_reader.lines())
    }
}

fn main() -> io::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let log_path = "/home/tomas/workspaces/frinx/odl/autorelease/distribution/distribution-karaf/target/assembly/data/log/karaf.log";
    let mut tail = Tail::new(log_path)?;
    // discard old logs
    get_last_log(&mut tail);

    let min_duration = Duration::from_secs(0);
    let max_duration = Duration::from_secs(2); // automatically cut down if successfully mounted

    loop {
        let mut sleep_duration = min_duration;
        while sleep_duration < max_duration {
            mount();
            sleep_duration += Duration::from_millis(10);

            if wait_for_prompt(&mut tail) {
                info!("Will sleep {}ms", sleep_duration.as_millis());
                thread::sleep(sleep_duration);
                if get_last_log(&mut tail).contains(&LogResult::DeviceSuccessfullyMounted) {
                    // too late, restart from 0
                    sleep_duration = min_duration;
                }
            } else {
                info!("Will sleep {}ms", max_duration.as_millis());
                thread::sleep(max_duration);
            }
            wait_for_unmount(&mut tail);
            info!("Unmounted, executing healthcheck mount");
            // another mount with long wait to make sure we did not enter the race on previous line
            mount();
            healthcheck(&mut tail);
            wait_for_unmount(&mut tail);
        }
    }
}
