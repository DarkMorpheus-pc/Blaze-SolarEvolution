use std::path::PathBuf;

extern "C" {
    fn getuid() -> u32;
}

pub fn current_uid() -> u32 {
    unsafe { getuid() }
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

pub fn get_solar_runtime_dir() -> PathBuf {
    if let Some(runtime_dir) = non_empty_env("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join("solar")
    } else {
        let uid = unsafe { getuid() };
        PathBuf::from(format!("/run/user/{}/solar", uid))
    }
}

pub fn get_solar_config_dir() -> PathBuf {
    if let Some(config_home) = non_empty_env("XDG_CONFIG_HOME") {
        PathBuf::from(config_home).join("solarui")
    } else if let Some(home) = non_empty_env("HOME") {
        PathBuf::from(home).join(".config/solarui")
    } else {
        PathBuf::from("/tmp/solarui")
    }
}

pub fn get_solar_core_socket_path() -> PathBuf {
    get_solar_runtime_dir().join("core.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_xdg_vars_fall_back_to_absolute_paths() {
        let prev_rt = std::env::var_os("XDG_RUNTIME_DIR");
        let prev_cfg = std::env::var_os("XDG_CONFIG_HOME");
        let prev_home = std::env::var_os("HOME");

        std::env::set_var("XDG_RUNTIME_DIR", "");
        std::env::set_var("XDG_CONFIG_HOME", "");
        std::env::set_var("HOME", "");
        assert!(get_solar_runtime_dir().is_absolute());
        assert!(get_solar_config_dir().is_absolute());
        assert!(get_solar_core_socket_path().is_absolute());

        match prev_rt {
            Some(v) => std::env::set_var("XDG_RUNTIME_DIR", v),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }
        match prev_cfg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}
