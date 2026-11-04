//! Argument handling shared by the examples: `--host H[:port]` (TCP) or
//! `--usb [--serial S]` (needs `--features usb`), plus example options.

#![allow(dead_code)]

use nebulosa::Device;

pub struct Args {
    args: Vec<String>,
}

impl Args {
    pub fn new() -> Self {
        Self { args: std::env::args().skip(1).collect() }
    }

    pub fn flag(&self, name: &str) -> bool {
        self.args.iter().any(|a| a == name)
    }

    pub fn value(&self, name: &str) -> Option<String> {
        self.args.iter().position(|a| a == name).and_then(|i| self.args.get(i + 1).cloned())
    }

    pub fn num<T: std::str::FromStr>(&self, name: &str, default: T) -> T {
        match self.value(name) {
            Some(v) => v.parse().unwrap_or_else(|_| usage(&format!("bad value for {name}: {v}"))),
            None => default,
        }
    }

    pub fn connect(&self) -> Device {
        let r = if self.flag("--usb") {
            usb(self.value("--serial"))
        } else if let Some(h) = self.value("--host") {
            Device::tcp(&h)
        } else {
            usage("give --host H or --usb")
        };
        r.unwrap_or_else(|e| {
            eprintln!("connect: {e}");
            std::process::exit(1)
        })
    }
}

#[cfg(feature = "usb")]
fn usb(serial: Option<String>) -> nebulosa::Result<Device> {
    Device::usb(match serial {
        Some(s) => nebulosa::UsbSelector::Serial(s),
        None => nebulosa::UsbSelector::Any,
    })
}

#[cfg(not(feature = "usb"))]
fn usb(_serial: Option<String>) -> nebulosa::Result<Device> {
    usage("built without USB: run with --features usb")
}

pub fn usage(msg: &str) -> ! {
    eprintln!("{msg}\nusage: <example> (--host H[:port] | --usb [--serial S]) [options]");
    std::process::exit(2)
}
