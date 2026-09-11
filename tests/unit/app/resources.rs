use super::*;
use crate::app::Args;
use clap::Parser;

#[test]
fn configuration_presets_keep_defaults_and_explicit_overrides() {
    let default = Args::try_parse_from(["wcode"]).unwrap();
    assert_eq!(
        default.resources.resolve().unwrap(),
        resource::ResourceLimits::new(10.0, 512, 32).unwrap()
    );
    for (preset, memory, slots, cpu) in [
        ("balanced", 512, 32, 10.0),
        ("fast", 1024, 64, 10.0),
        ("light", 256, 16, 5.0),
    ] {
        let args = Args::try_parse_from(["wcode", "--performance", preset]).unwrap();
        let limits = args.resources.resolve().unwrap();
        assert_eq!(limits.max_memory_bytes, memory * 1024 * 1024);
        assert_eq!(limits.requested_parallel_tools, slots);
        assert_eq!(limits.max_cpu_percent, cpu);
        assert!(!args.full_access && !args.allow_risky_exec && !args.allow_destructive_writes);
    }
    for argv in [
        vec![
            "wcode",
            "--performance",
            "fast",
            "--max-memory-mb",
            "256",
            "-j",
            "7",
        ],
        vec![
            "wcode",
            "-j",
            "7",
            "--max-memory-mb",
            "256",
            "--performance",
            "fast",
        ],
    ] {
        let args = Args::try_parse_from(argv).unwrap();
        let limits = args.resources.resolve().unwrap();
        assert_eq!(limits.max_memory_bytes, 256 * 1024 * 1024);
        assert_eq!(limits.effective_parallel_tools, 7);
        assert_eq!(limits.max_cpu_percent, 10.0);
    }
}

#[test]
fn configuration_preview_reports_effective_limits_and_origins_without_activation() {
    let args = Args::try_parse_from([
        "wcode",
        "mcp-stdio",
        "--performance",
        "fast",
        "--max-memory-mb",
        "128",
        "--read-only",
        "--show-config",
        "-w",
        "project with spaces",
    ])
    .unwrap();
    let preview = args.configuration_preview().unwrap();
    assert_eq!(preview["preview"], true);
    assert_eq!(preview["runtime_started"], false);
    assert_eq!(preview["resources"]["preset"], "fast");
    assert_eq!(
        preview["resources"]["effective"]["requested_parallel_tools"],
        64
    );
    assert_eq!(
        preview["resources"]["effective"]["effective_parallel_tools"],
        16
    );
    assert_eq!(
        preview["resources"]["sources"]["max_memory_mb"],
        "command_line"
    );
    assert_eq!(preview["resources"]["sources"]["parallel_tools"], "preset");
    assert_eq!(preview["workspace_paths"][0], "project with spaces");
    assert_eq!(preview["permissions"]["write_enabled"], false);
    assert_eq!(preview["connection"]["mode"], "stdio");
}

#[test]
fn setup_launch_options_round_trip_without_persisting_authority() {
    for preset in ["balanced", "fast", "light"] {
        let source = Args::try_parse_from([
            "wcode",
            "setup",
            "--performance",
            preset,
            "--max-cpu-percent",
            "12.5",
            "--max-memory-mb",
            "256",
            "-j",
            "7",
            "--read-only",
            "--no-exec",
            "--no-semantic",
        ])
        .unwrap();
        let launch = source.setup_launch_args().unwrap();
        let parsed =
            Args::try_parse_from(std::iter::once("wcode".to_owned()).chain(launch.clone()))
                .unwrap();
        assert_eq!(
            parsed.resources.resolve().unwrap(),
            source.resources.resolve().unwrap()
        );
        assert!(!parsed.allow_write && !parsed.allow_exec && !parsed.allow_semantic);
        assert!(!parsed.full_access && !parsed.allow_risky_exec);
        assert_eq!(
            source.configuration_preview().unwrap()["setup_launch"]["args"],
            json!(launch)
        );
    }
    let ordinary = Args::try_parse_from(["wcode", "setup"]).unwrap();
    assert_eq!(ordinary.setup_launch_args().unwrap(), ["mcp-stdio"]);
    let broad = Args::try_parse_from(["wcode", "setup", "--full-access"]).unwrap();
    assert!(broad.setup_launch_args().is_err());
}

#[test]
fn configuration_rejects_invalid_overrides_and_contradictory_modes() {
    assert!(Args::try_parse_from(["wcode", "--performance", "turbo"]).is_err());
    for (option, value) in [
        ("--max-memory-mb", "64"),
        ("--max-cpu-percent", "NaN"),
        ("--max-cpu-percent", "0"),
        ("--max-parallel-tools", "0"),
        ("--max-parallel-tools", "257"),
    ] {
        let args = Args::try_parse_from(["wcode", option, value]).unwrap();
        assert!(args.resources.resolve().is_err());
    }
    for restriction in ["--read-only", "--no-exec", "--no-semantic"] {
        assert!(Args::try_parse_from(["wcode", "--full-access", restriction]).is_err());
        assert!(
            Args::try_parse_from(["wcode", "mcp-stdio", restriction, "--full-access"]).is_err()
        );
    }
    assert!(Args::try_parse_from([
        "wcode",
        "--no-tunnel",
        "--public-url",
        "https://example.test"
    ])
    .is_err());
    let invalid_url =
        Args::try_parse_from(["wcode", "--public-url", "not a URL", "--show-config"]).unwrap();
    assert!(invalid_url.configuration_preview().is_err());
}
