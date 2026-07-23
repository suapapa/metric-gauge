fn main() {
    linker_be_nice();
    // make sure linkall.x is the last linker script (otherwise might cause problems with flip-link)
    println!("cargo:rustc-link-arg=-Tlinkall.x");

    // Compile-time configuration (passed via ENV when building/flashing).
    // Example:
    //   SSID=myssid PASS=secret \
    //   GUAGE1_PROM_METRIC=https://node1.homin.dev/metrics \
    //   GUAGE2_PROM_METRIC=https://node2.homin.dev/metrics \
    //   cargo run --release
    emit_env("SSID", None);
    emit_env("PASS", Some("PASSWORD")); // PASSWORD accepted as alias
    emit_env("GUAGE1_PROM_METRIC", Some("GAUGE1_PROM_METRIC"));
    emit_env("GUAGE2_PROM_METRIC", Some("GAUGE2_PROM_METRIC"));
}

fn emit_env(primary: &str, alias: Option<&str>) {
    println!("cargo:rerun-if-env-changed={primary}");
    if let Some(alias) = alias {
        println!("cargo:rerun-if-env-changed={alias}");
    }

    let value = std::env::var(primary)
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| alias.and_then(|a| std::env::var(a).ok().filter(|v| !v.is_empty())))
        .unwrap_or_default();

    if value.is_empty() {
        eprintln!("cargo:warning={primary} is unset; firmware will use an empty placeholder");
    }
    println!("cargo:rustc-env={primary}={value}");
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                what if what.starts_with("_defmt_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`"
                    );
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                what if what.starts_with("esp_rtos_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `esp-radio` has no scheduler enabled. Make sure you have initialized `esp-rtos` or provided an external scheduler."
                    );
                    eprintln!();
                }
                "embedded_test_linker_file_not_added_to_rustflags" => {
                    eprintln!();
                    eprintln!(
                        "💡 `embedded-test` not found - make sure `embedded-test.x` is added as a linker script for tests"
                    );
                    eprintln!();
                }
                "free"
                | "malloc"
                | "calloc"
                | "get_free_internal_heap_size"
                | "malloc_internal"
                | "realloc_internal"
                | "calloc_internal"
                | "free_internal" => {
                    eprintln!();
                    eprintln!(
                        "💡 Did you forget the `esp-alloc` dependency or didn't enable the `compat` feature on it?"
                    );
                    eprintln!();
                }
                _ => (),
            },
            // we don't have anything helpful for "missing-lib" yet
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }

    println!(
        "cargo:rustc-link-arg=--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
}
