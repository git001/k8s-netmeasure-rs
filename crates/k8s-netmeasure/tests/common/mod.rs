//! Helpers shared by the end-to-end tests.
#![allow(dead_code)] // each test crate uses a different subset

use std::net::TcpListener;

use assert_cmd::Command;

pub fn bin() -> Command {
    let mut c = Command::cargo_bin("k8s-netmeasure").expect("binary is built");
    // Never inherit the developer's or CI's K8S_NETMEASURE_* variables.
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("K8S_NETMEASURE_") {
            c.env_remove(k);
        }
    }
    c
}

pub fn stdout(out: &assert_cmd::assert::Assert) -> String {
    String::from_utf8(out.get_output().stdout.clone()).expect("utf-8")
}

pub fn stderr(out: &assert_cmd::assert::Assert) -> String {
    String::from_utf8(out.get_output().stderr.clone()).expect("utf-8")
}

pub fn closed_port() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    drop(l);
    addr
}
