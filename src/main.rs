#[macro_use]
extern crate log;
use std::process::Command;
use std::time::Duration;
use std::thread;
use std::str;

fn mount(idx: u32) {
    let device = format!("ME_CLI_{}", idx);
    let output = Command::new("curl")
        .args(&[
            "-v", "-H", "Content-Type: application/json",
            &format!("admin:admin@localhost:8181/restconf/config/network-topology:network-topology/topology/cli/node/{}", device),
            "-X", "PUT", "-d",
            &(r#"{
                "network-topology:node" :
                {
                  "network-topology:node-id" : ""#.to_owned() + &device + r#"",
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
            }"#)
        ])
        .output();
    let output = output.unwrap();
    assert!(
        output.status.success(),
        "Operation mount failed. {:?}",
        output
    );
    let stdout = str::from_utf8(&output.stdout).unwrap();
    debug!("[{}] mount stdout: {}", idx, stdout);
    info!("[{}] mount ok", idx);
}

// check that get oper does not contain IllegalStateException:
/*
{
    "network-topology": {
        "topology": [
            {
                "topology-id": "cli",
                "node": [
                    {
                        "node-id": "ME_CLI",
                        "cli-topology:connection-status": "unable-to-connect",
                        "cli-topology:connected-message": "Unable to expose mountpoint: java.lang.IllegalStateException: Mount point already exists",
                        "cli-topology:default-error-patterns": {
                            "error-pattern": [
                                "(^|\\n)% (?i)Ambiguous command(?-i).*",
                                "(^|\\n)% (?i)invalid input(?-i).*",
                                "(^|\\n)% (?i)Incomplete command(?-i).*",
                                "(^|\\n)\\s+\\^.*"
                            ]
                        },
                        "cli-topology:default-commit-error-patterns": {
                            "commit-error-pattern": [
                                "(^|\\n)% (?i)Failed(?-i).*"
                            ]
                        }
                    }
                ]
            },

*/
fn check_mount_status() {
    let output = Command::new("curl")
        .args(&[
            "-v", "-H", "Content-Type: application/json",
            "admin:admin@localhost:8181/restconf/operational/network-topology:network-topology/topology/cli/",
            "-X", "GET"
        ])
        .output();
    let output = output.unwrap();
    assert!(
        output.status.success(),
        "Operation mount failed. {:?}",
        output
    );
    let stdout = str::from_utf8(&output.stdout).unwrap();
    debug!("check_mount_status stdout: {}", stdout);
    if stdout.contains("IllegalStateException") {
        panic!("check_mount_status failed: {}", stdout);
    }
}

fn unmount(idx: u32) {
    let output = Command::new("curl")
        .args(&[
            "-v", "-H", "Content-Type: application/json",
            &format!("admin:admin@localhost:8181/restconf/config/network-topology:network-topology/topology/cli/node/ME_CLI_{}", idx),
            "-X", "DELETE"
        ])
        .output();
    let output = output.unwrap();
    assert!(
        output.status.success(),
        "Operation mount failed. {:?}",
        output
    );
    info!("[{}] unmount ok", idx);
}

fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    let min_duration = Duration::from_millis(10_000);
    let max_duration = Duration::from_millis(15_000);
    let max_devices = 1;
    loop {
        let mut sleep_duration = min_duration;
        while sleep_duration < max_duration {
            info!("Will sleep {}ms", sleep_duration.as_millis());
            for idx in 0..max_devices {
                mount(idx);
            }
            thread::sleep(sleep_duration);
            check_mount_status();
            for idx in 0..max_devices {
                unmount(idx);
            }
            sleep_duration += Duration::from_millis(100);
        }
    }
}
