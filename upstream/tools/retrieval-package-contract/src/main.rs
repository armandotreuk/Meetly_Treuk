use retrieval_package_contract::{validate_expanded, validate_static, ContractResult};
use std::{env, fs, path::PathBuf};

fn run() -> ContractResult<()> {
    let mut args = env::args_os().skip(1);
    let first = args.next().ok_or("usage")?;
    let static_only = first == "--static";
    let config = PathBuf::from(if static_only {
        args.next().ok_or("usage")?
    } else {
        first
    });
    if args.next().is_some() {
        return Err("usage");
    }
    let metadata = fs::symlink_metadata(&config).map_err(|_| "config_unreadable")?;
    if metadata.file_type().is_symlink() {
        return Err("linked_config");
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("linked_config");
        }
    }
    let config = dunce::canonicalize(config).map_err(|_| "config_unreadable")?;
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(&config).map_err(|_| "config_unreadable")?)
            .map_err(|_| "invalid_config")?;
    env::set_current_dir(config.parent().ok_or("config_unreadable")?)
        .map_err(|_| "config_unreadable")?;
    let resources = &json["bundle"]["resources"];
    if static_only {
        validate_static(resources)?;
    } else {
        validate_expanded(resources)?;
    }
    Ok(())
}

fn main() {
    match run() {
        Ok(()) => println!("package-resource-contract: status=passed"),
        Err(code) => {
            // No source paths, configuration values, or error-chain payloads.
            eprintln!("package-resource-contract: status=failed code={code}");
            std::process::exit(1);
        }
    }
}
