#[macro_use]
extern crate log;
use std::process::Command;
use std::str;
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

enum WaitForPromptResult {
    PromptResolved,
    Timeout,
}

// Monitor log file. Wait until the prompt is resolved. Normally this ends in ~10s.
// Sequenced messages:
// "Prompt resolved"
// "Exposing mountpoint under"
// hopefully this state will not execute:
// "Device successfully mounted" // too late
fn wait_for_prompt() -> WaitForPromptResult {
    let now = SystemTime::now();
    let max_duration = Duration::from_secs(15);
    while now.elapsed().unwrap() < max_duration {
        let output = Command::new("tail")
        .args(&[
            "-n", "20",
            "/home/tomas/workspaces/frinx/odl/autorelease/distribution/distribution-karaf/target/assembly/data/log/karaf.log"
        ])
        .output();
        let output = output.unwrap();
        assert!(output.status.success(), "tail failed. {:?}", output);
        let stdout = str::from_utf8(&output.stdout).unwrap();
        trace!("wait_for_prompt stdout: {}", stdout);
        if stdout.contains("Prompt resolved") {
            info!("wait_for_prompt - PromptResolved");
            return WaitForPromptResult::PromptResolved;
        }
        thread::sleep(Duration::from_millis(10));
    }
    info!("wait_for_prompt - Timeout");
    return WaitForPromptResult::Timeout;
}

#[derive(Debug)]
enum AfterSleepLogResult {
    PromptResolved,
    ExposingMountpointUnder,
    DeviceSuccessfullyMounted,
    MountPointAlreadyExistsFailure,
    Unknown,
}

fn check_logs_after_sleep() -> AfterSleepLogResult {
    let output = Command::new("tail")
    .args(&[
        "-n", "50",
        "/home/tomas/workspaces/frinx/odl/autorelease/distribution/distribution-karaf/target/assembly/data/log/karaf.log"
    ])
    .output();
    let output = output.unwrap();
    assert!(output.status.success(), "tail failed. {:?}", output);
    let stdout = str::from_utf8(&output.stdout).unwrap();
    trace!("check_logs_after_sleep stdout: {}", stdout);
    let result = if stdout.contains("Mount point already exists") {
        AfterSleepLogResult::MountPointAlreadyExistsFailure
    } else if stdout.contains("Device successfully mounted") {
        AfterSleepLogResult::DeviceSuccessfullyMounted
    } else if stdout.contains("Exposing mountpoint under") {
        AfterSleepLogResult::ExposingMountpointUnder
    } else if stdout.contains("Prompt resolved") {
        AfterSleepLogResult::PromptResolved
    } else {
        AfterSleepLogResult::Unknown
    };
    debug!("check_logs_after_sleep - {:?}", result);
    result
}

fn unmount() {
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
    info!("unmount ok");
}

fn healthcheck() -> bool {
    if let WaitForPromptResult::PromptResolved = wait_for_prompt() {
        let now = SystemTime::now();
        let max_duration = Duration::from_secs(10);
        while now.elapsed().unwrap() < max_duration {
            match check_logs_after_sleep() {
                AfterSleepLogResult::DeviceSuccessfullyMounted => {
                    return true;
                }
                AfterSleepLogResult::MountPointAlreadyExistsFailure => {
                    panic!("Found the error state");
                }
                _ => {}
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    return false;
}

fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    let min_duration = Duration::from_millis(70);
    let max_duration = Duration::from_millis(200); // automatically cut down

    loop {
        let mut sleep_duration = min_duration;
        while sleep_duration < max_duration {
            unmount();
            mount();
            if let WaitForPromptResult::PromptResolved = wait_for_prompt() {
                info!("Will sleep {}ms", sleep_duration.as_millis());
                thread::sleep(sleep_duration);
                match check_logs_after_sleep() {
                    AfterSleepLogResult::DeviceSuccessfullyMounted => {
                        break;
                    } // too late, restart
                    AfterSleepLogResult::MountPointAlreadyExistsFailure => {
                        panic!("Found the error state");
                    }
                    _ => {}
                }
            }
            unmount();
            info!("Executing healthcheck mount");
            // another mount with long wait to make sure we did not enter the race on previous line
            mount();
            let healthcheck_result = healthcheck();
            info!("Healthcheck passed?: {}", healthcheck_result);

            sleep_duration += Duration::from_millis(10);
        }
    }
}
