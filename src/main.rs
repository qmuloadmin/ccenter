use std::collections::HashMap;

use clap::{Parser, Subcommand};
use cmd_lib::*;
use serde_json::{from_str, to_string, Value};
use std::process;
use tokio::task::JoinSet;
use tokio::time::{sleep, Duration};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Turn up the volume of default sink by percentage
    Volup,
    /// Turn down the volume of default sink by percentage
    Voldn,
    /// Toggle mute of the defeault sink
    Volmute,
    /// Update list of sinks in EWW bar
    UpdateSinks,
    /// Turn up the volume of the default source by percentage
    Micup,
    /// Turn down the volume of the default source by percentage
    Micdn,
    /// Toggle mute of the default source
    Micmute,
    /// Toggle Do Not Disturb
    Dnd,
	/// Increase brightness of default screen by percentage
	Brtup,
	/// Decrease brightness of default screen by percentage
	Brtdn,
}

#[tokio::main]
async fn main() -> CmdResult {
    let cli = Cli::parse();
	match cli.command {
        Command::UpdateSinks => set_eww_sinks().await?,
		Command::Volup => vol_up().await?,
		Command::Voldn => vol_dn().await?,
		Command::Volmute => vol_mute().await?,
		Command::Micmute => mic_mute().await?,
		Command::Brtdn => bright_down().await?,
		Command::Brtup => bright_up().await?,
		_ => ()
    };
    Ok(())
}

type DebouncedCommand = (&'static str, &'static str);

async fn debounce(commands: &[DebouncedCommand]) -> CmdResult {
	let pid = process::id();
	let mut tasks = JoinSet::new();
	for (command, value) in commands.into_iter() {
		run_cmd!(eww update ccenter-$command=$pid)?;
		let command = command.to_owned();
		let value = value.to_owned();
		tasks.spawn(async move {
			sleep(Duration::from_secs(5)).await;
			let last_pid = run_fun!(eww get "ccenter-$command").unwrap();
			if last_pid.parse::<u32>().unwrap() == pid {
				run_cmd!(eww update $command=$value).unwrap();
			}
		});
	}
	while let Some(res) = tasks.join_next().await {
		res?;
	}
	Ok(())
}

async fn bright_up() -> CmdResult {
	run_cmd!(eww update showside="true")?;
	run_cmd!(eww update bright="true")?;
	run_cmd!(brightnessctl set +10%)?;
	let level = run_fun!(brightnessctl -m | awk -F, "{print substr($4, 0, length($4)-1)}" | tr -d "%")?;
	run_cmd!(eww update current-brightness=$level)?;
	debounce(&[("bright", "false"), ("showside", "false")]).await
}

async fn bright_down() -> CmdResult {
	run_cmd!(eww update showside="true")?;
	run_cmd!(eww update bright="true")?;
	run_cmd!(brightnessctl set 10%-)?;
	let level = run_fun!(brightnessctl -m | awk -F, "{print substr($4, 0, length($4)-1)}" | tr -d "%")?;
	run_cmd!(eww update current-brightness=$level)?;
	debounce(&[("bright", "false"), ("showside", "false")]).await
}

async fn mic_mute() -> CmdResult {
	run_cmd!(eww update showside="true")?;
	let muted = run_fun!(pactl get-source-mute @DEFAULT_SOURCE@ | awk "{print $2}")?;
	if muted == "yes" {
		run_cmd!(pactl set-source-mute @DEFAULT_SOURCE@ no)?;
		run_cmd!(eww update micmute="no")?;
	} else {
		run_cmd!(pactl set-source-mute @DEFAULT_SOURCE@ yes)?;
		run_cmd!(eww update micmute="yes")?;
	};
	debounce(&[("showside", "false")]).await
}

async fn vol_mute() -> CmdResult {
	run_cmd!(eww update showside="true")?;
	let muted = run_fun!(pactl get-sink-mute @DEFAULT_SINK@ | awk "{print $2}")?;
	if muted == "yes" {
		run_cmd!(pactl set-sink-mute @DEFAULT_SINK@ no)?;
		run_cmd!(eww update mute="no")?;
	} else {
		run_cmd!(pactl set-sink-mute @DEFAULT_SINK@ yes)?;
		run_cmd!(eww update mute="yes")?;
	};
	debounce(&[("showside", "false")]).await
}

async fn vol_up() -> CmdResult {
	run_cmd!(eww update showside="true")?;
	run_cmd!(eww update volume="true")?;
	run_cmd!(pactl set-sink-volume @DEFAULT_SINK@ +5%)?;
	run_cmd!(pactl play-sample bell-terminal)?;
	let volume = run_fun!(pactl get-sink-volume @DEFAULT_SINK@ | grep Volume | awk "{print $5}" | tr -d "%")?;
	run_cmd!(eww update current-volume=$volume)?;
	debounce(&[("volume", "false"), ("showside", "false")]).await
}

async fn vol_dn() -> CmdResult {
	run_cmd!(eww update showside="true")?;
	run_cmd!(eww update volume="true")?;
	run_cmd!(pactl set-sink-volume @DEFAULT_SINK@ -5%)?;
	run_cmd!(pactl play-sample bell-terminal)?;
	let volume = run_fun!(pactl get-sink-volume @DEFAULT_SINK@ | grep Volume | awk "{print $5}" | tr -d "%")?;
	run_cmd!(eww update current-volume=$volume)?;
	debounce(&[("volume", "false"), ("showside", "false")]).await?;
	Ok(())
}

async fn set_eww_sinks() -> CmdResult {
    let json = run_fun! (
        pactl -fjson list sinks | jq "[.[] | {name: .name, index: .index, desc: (.description | (.[-24:])), bus: .properties.\"device.bus\", icon: .properties.\"device.icon_name\", available: .ports[].availability}]"
    )?;
    let active = run_fun! (
        pactl -fjson get-default-sink
    )?;
    let mut sinks = Vec::new();
    let json: Value = from_str(&json).unwrap();
    for sink in json.as_array().unwrap() {
        if sink["available"] == "not available" {
            continue;
        }
        let mut data: HashMap<&str, Value> = HashMap::new();
        let name = sink["name"].as_str().unwrap();
        let icon = sink["icon"].as_str().unwrap();
        let bus = sink["bus"].as_str().unwrap();
        let desc = sink["desc"].as_str().unwrap();
        data.insert(
            "icon",
            Value::from(match desc {
                x if x.contains("HDMI") => "",
                x if x.contains("DisplayPort") => "",
                _ => match icon {
                    x if x.contains("headset") => "",
                    x if x.contains("headphones") => "",
                    _ => match bus {
                        x if x == "bluetooth" => "",
                        x if x == "usb" => "",
                        x if x == "pci" => "",
                        _ => "",
                    },
                },
            }),
        );
        if name == active {
            data.insert("active", Value::from(true));
        } else {
            data.insert("active", Value::from(false));
        }
        data.insert("name", Value::from(name));
        data.insert("desc", Value::from(desc));
        sinks.push(data);
    }
    let sinks = to_string(&sinks).unwrap();
    run_cmd! (
        eww update pa-sinks=$sinks
    )?;
    Ok(())
}
