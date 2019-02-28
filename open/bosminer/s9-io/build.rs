// Copyright (C) 2019  braiins systems s.r.o.
//
// This file is part of rurminer.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

const SRC_DIR: &'static str = "src";

fn main() -> std::io::Result<()> {
    let current_dir = env::current_dir()?;
    let out_dir = env::var("OUT_DIR").unwrap();

    if Path::new(SRC_DIR).is_dir() {
        fs::remove_dir_all(SRC_DIR)?;
    }
    fs::create_dir(SRC_DIR)?;
    Command::new("svd2rust")
        .args(&["--target", "none", "-i"])
        .arg(Path::new(&current_dir).join("fpga-io.xml"))
        .current_dir(Path::new(&out_dir))
        .status()?;
    Command::new("form")
        .args(&["-i", "lib.rs", "-o"])
        .arg(Path::new(&current_dir).join(SRC_DIR))
        .current_dir(Path::new(&out_dir))
        .status()?;
    Command::new("rustfmt")
        .arg(Path::new(&current_dir).join("src").join("lib.rs"))
        .status()?;

    // rebuild lib.rs only if fpga-io.xml is changed
    print!("cargo:rerun-if-changed=fpga-io.xml");
    Ok(())
}
