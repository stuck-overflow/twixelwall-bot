mod token_storage;

use image::io::Reader as ImageReader;
use image::{Pixel, Rgba};
use log::{debug, trace, LevelFilter};
use serde::Deserialize;
use simple_logger::SimpleLogger;
use std::convert::TryFrom;
use std::fs;
use structopt::StructOpt;
use tempfile::tempdir;
use token_storage::CustomTokenStorage;
use twitch_api2::twitch_oauth2::Scope;
use twitch_irc::login::{RefreshingLoginCredentials, TokenStorage};
use twitch_irc::message::ServerMessage;
use twitch_irc::{ClientConfig, TCPTransport, TwitchIRCClient};

#[derive(Clone, Deserialize)]
struct TwixelWallBotConfig {
    twitch: TwitchConfig,
    twixel: TwixelConfig,
}

#[derive(Clone, Deserialize)]
struct TwitchConfig {
    token_filepath: String,
    login_name: String,
    channel_name: String,
    client_id: String,
    secret: String,
}

#[derive(Clone, Deserialize)]
struct TwixelConfig {
    img_filepath: String,
    width: u32,
    height: u32,
}

// Command-line arguments for the tool.
#[derive(StructOpt)]
struct Cli {
    /// Log level
    #[structopt(short, long, case_insensitive = true, default_value = "INFO")]
    log_level: LevelFilter,

    /// Twitch credential files.
    #[structopt(short, long, default_value = "twixelwall-bot.toml")]
    config_file: String,
}

#[derive(Debug)]
struct Command {
    x: u32,
    y: u32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl TryFrom<String> for Command {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let r: Result<Vec<_>, _> = value.split(' ').map(|v| v.parse::<u32>()).collect();
        println!("{:?}", r);
        match r {
            Ok(v) => {
                if !(5..7).contains(&v.len()) {
                    return Err("too many args");
                }
                if v[2] > 255 || v[3] > 255 || v[4] > 255 {
                    return Err("invalid r g b");
                }
                Ok(Command {
                    x: v[0],
                    y: v[1],
                    r: v[2] as u8,
                    g: v[3] as u8,
                    b: v[4] as u8,
                    a: if v.len() == 6 { v[5] as u8 } else { 255 },
                })
            }
            Err(_) => Err("error parsing"),
        }
    }
}

#[tokio::main]
pub async fn main() {
    let args = Cli::from_args();
    SimpleLogger::new()
        .with_level(args.log_level)
        .init()
        .unwrap();

    let config = match fs::read_to_string(&args.config_file) {
        Ok(config) => config,
        Err(e) => {
            eprintln!(
                "Error opening the configuration file {}: {}",
                args.config_file, e
            );
            eprintln!("Create the file or use the --config_file flag to specify an alternative file location");
            return;
        }
    };

    let config: TwixelWallBotConfig = match toml::from_str(&config) {
        Ok(config) => config,
        Err(e) => {
            eprintln!(
                "Error parsing configuration file {}: {}",
                args.config_file, e
            );
            return;
        }
    };

    let mut token_storage = CustomTokenStorage {
        token_checkpoint_file: config.twitch.token_filepath.clone(),
    };

    // If we have some errors while loading the stored token, e.g. if we never
    // stored one before or it's unparsable, go through the authentication
    // workflow.
    if let Err(_) = token_storage.load_token().await {
        let user_token = twitch_oauth2_auth_flow::auth_flow(
            &config.twitch.client_id,
            &config.twitch.secret,
            Some(vec![Scope::ChatRead]),
        );
        token_storage
            .write_twitch_oauth2_user_token(
                &user_token,
                Some(oauth2::ClientSecret::new(config.twitch.secret.clone())),
            )
            .unwrap();
    }

    let irc_config = ClientConfig::new_simple(RefreshingLoginCredentials::new(
        config.twitch.login_name.clone(),
        config.twitch.client_id.clone(),
        config.twitch.secret.clone(),
        token_storage.clone(),
    ));

    let (mut incoming_messages, twitch_irc_client) =
        TwitchIRCClient::<TCPTransport, _>::new(irc_config);

    // join a channel
    twitch_irc_client.join(config.twitch.channel_name.to_owned());

    let join_handle = tokio::spawn(async move {
        while let Some(message) = incoming_messages.recv().await {
            trace!("{:?}", message);
            match message {
                ServerMessage::Privmsg(msg) => {
                    let command = match Command::try_from(msg.message_text) {
                        Err(_) => continue,
                        Ok(c) => c,
                    };
                    debug!("{:?}", command);
                    if command.x >= config.twixel.width || command.y >= config.twixel.height {
                        continue;
                    }
                    let mut img = ImageReader::open(config.twixel.img_filepath.to_owned())
                        .unwrap()
                        .decode()
                        .unwrap()
                        .to_rgba8();
                    img.get_pixel_mut(command.x, command.y)
                        .blend(&Rgba([command.r, command.g, command.b, command.a]));
                    let tmpdir = tempdir().unwrap();
                    let tmpfile = tmpdir.path().join("img.png");
                    if let Err(e) = img.save(&tmpfile) {
                        eprintln!("Unable to save to tmpfile: {}", e);
                        continue;
                    }

                    fs::rename(tmpfile, &config.twixel.img_filepath).unwrap();
                }
                _ => continue,
            }
        }
    });

    // keep the tokio executor alive.
    // If you return instead of waiting the background task will exit.
    join_handle.await.unwrap();
}
