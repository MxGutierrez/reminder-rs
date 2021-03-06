use custom_error::custom_error;

use regex_command_attr::command;

use serenity::{
    http::CacheHttp,
    client::Context,
    model::{
        misc::Mentionable,
        id::{
            UserId,
            ChannelId,
            GuildId,
        },
        channel::{
            Message,
        },
    },
    framework::standard::CommandResult,
};

use tokio::process::Command;

use crate::{
    models::{
        ChannelData,
        GuildData,
        UserData,
        Reminder,
        Timer,
    },
    check_subscription,
    SQLPool,
    time_parser::TimeParser,
    framework::SendIterator,
};

use chrono::NaiveDateTime;

use chrono_tz::Etc::UTC;

use rand::{
    rngs::OsRng,
    RngCore,
};

use sqlx::{
    Type,
    MySql,
    MySqlPool,
    encode::Encode,
};

use std::str::from_utf8;

use num_integer::Integer;

use std::{
    convert::TryInto,
    default::Default,
    env,
    string::ToString,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use regex::Regex;

use serde_json::json;
use serenity::cache::Cache;

lazy_static! {
    static ref REGEX_CHANNEL: Regex = Regex::new(r#"^\s*<#(\d+)>\s*$"#).unwrap();

    static ref REGEX_CHANNEL_USER: Regex = Regex::new(r#"^\s*<(#|@)(?:!)?(\d+)>\s*$"#).unwrap();

    static ref MIN_INTERVAL: i64 = env::var("MIN_INTERVAL").ok().map(|inner| inner.parse::<i64>().ok()).flatten().unwrap_or(600);

    static ref MAX_TIME: i64 = env::var("MAX_TIME").ok().map(|inner| inner.parse::<i64>().ok()).flatten().unwrap_or(60*60*24*365*50);
}

static CHARACTERS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";


#[command]
#[supports_dm(false)]
#[permission_level(Restricted)]
async fn pause(ctx: &Context, msg: &Message, args: String) -> CommandResult {
    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let user_data = UserData::from_user(&msg.author, &ctx, &pool).await.unwrap();
    let mut channel = ChannelData::from_channel(msg.channel(&ctx).await.unwrap(), &pool).await.unwrap();

    if args.is_empty() {
        channel.paused = !channel.paused;
        channel.paused_until = None;

        channel.commit_changes(&pool).await;

        if channel.paused {
            let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "pause/paused_indefinite").await).await;
        }
        else {
            let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "pause/unpaused").await).await;
        }
    }
    else {
        let parser = TimeParser::new(args, user_data.timezone.parse().unwrap());
        let pause_until = parser.timestamp();

        match pause_until {
            Ok(timestamp) => {
                channel.paused = true;
                channel.paused_until = Some(NaiveDateTime::from_timestamp(timestamp, 0));

                channel.commit_changes(&pool).await;

                let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "pause/paused_until").await).await;
            },

            Err(_) => {
                let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "pause/invalid_time").await).await;
            },
        }
    }

    Ok(())
}

#[command]
#[permission_level(Restricted)]
async fn offset(ctx: &Context, msg: &Message, args: String) -> CommandResult {
    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let user_data = UserData::from_user(&msg.author, &ctx, &pool).await.unwrap();

    if args.is_empty() {
        let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "offset/help").await).await;
    }
    else {
        let parser = TimeParser::new(args, user_data.timezone());

        if let Ok(displacement) = parser.displacement() {
            if let Some(guild) = msg.guild(&ctx).await {
                let guild_data = GuildData::from_guild(guild, &pool).await.unwrap();

                sqlx::query!(
                    "
UPDATE reminders
    INNER JOIN `channels`
        ON `channels`.id = reminders.channel_id
    SET
        reminders.`time` = reminders.`time` + ?
    WHERE channels.guild_id = ?
                    ", displacement, guild_data.id)
                    .execute(&pool)
                    .await
                    .unwrap();
            } else {
                sqlx::query!(
                    "
UPDATE reminders SET `time` = `time` + ? WHERE reminders.channel_id = ?
                    ", displacement, user_data.dm_channel)
                    .execute(&pool)
                    .await
                    .unwrap();
            }

            let response = user_data.response(&pool, "offset/success").await.replacen("{}", &displacement.to_string(), 1);

            let _ = msg.channel_id.say(&ctx, response).await;
        } else {
            let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "offset/invalid_time").await).await;
        }
    }

    Ok(())
}

#[command]
#[permission_level(Restricted)]
async fn nudge(ctx: &Context, msg: &Message, args: String) -> CommandResult {
    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let user_data = UserData::from_user(&msg.author, &ctx, &pool).await.unwrap();
    let mut channel = ChannelData::from_channel(msg.channel(&ctx).await.unwrap(), &pool).await.unwrap();

    if args.is_empty() {
        let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "nudge/invalid_time").await).await;
    }
    else {
        let parser = TimeParser::new(args, user_data.timezone.parse().unwrap());
        let nudge_time = parser.displacement();

        match nudge_time {
            Ok(displacement) => {
                if displacement < i16::MIN as i64 || displacement > i16::MAX as i64 {
                    let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "nudge/invalid_time").await).await;
                }
                else {
                    channel.nudge = displacement as i16;

                    channel.commit_changes(&pool).await;

                    let response = user_data.response(&pool, "nudge/success").await.replacen("{}", &displacement.to_string(), 1);

                    let _ = msg.channel_id.say(&ctx, response).await;
                }
            },

            Err(_) => {
                let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "nudge/invalid_time").await).await;
            },
        }
    }

    Ok(())
}

enum TimeDisplayType {
    Absolute,
    Relative,
}

struct LookFlags {
    pub limit: u16,
    pub show_disabled: bool,
    pub channel_id: Option<u64>,
    time_display: TimeDisplayType,
}

impl Default for LookFlags {
    fn default() -> Self {
        Self {
            limit: u16::MAX,
            show_disabled: true,
            channel_id: None,
            time_display: TimeDisplayType::Relative
        }
    }
}

impl LookFlags {
    fn from_string(args: &str) -> Self {
        let mut new_flags: Self = Default::default();

        for arg in args.split(' ') {
            match arg {
                "enabled" => {
                    new_flags.show_disabled = false;
                },

                "time" => {
                    new_flags.time_display = TimeDisplayType::Absolute;
                },

                param => {
                    if let Ok(val) = param.parse::<u16>() {
                        new_flags.limit = val;
                    }
                    else {
                        new_flags.channel_id = REGEX_CHANNEL.captures(&args)
                            .map(|cap| cap.get(1))
                            .flatten()
                            .map(|c| c.as_str().parse::<u64>().unwrap());
                    }
                }
            }
        }

        new_flags
    }

    pub fn display_time(&self, timestamp: u64) -> String {
        match self.time_display {
            TimeDisplayType::Absolute => {
                timestamp.to_string()
            },

            TimeDisplayType::Relative => {
                timestamp.to_string()
            },
        }
    }
}

#[command]
#[permission_level(Managed)]
async fn look(ctx: &Context, msg: &Message, args: String) -> CommandResult {
    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let user_data = UserData::from_user(&msg.author, &ctx, &pool).await.unwrap();

    let flags = LookFlags::from_string(&args);

    let enabled = if flags.show_disabled { "0,1" } else { "1" };

    let reminders = if let Some(guild_id) = msg.guild_id.map(|f| f.as_u64().to_owned()) {
        let channel_id = flags.channel_id.unwrap_or_else(|| msg.channel_id.as_u64().to_owned());

        sqlx::query_as!(Reminder,
            "
SELECT
    reminders.id, reminders.time, reminders.name, reminders.channel_id
FROM
    reminders
INNER JOIN
    channels
ON
    reminders.channel_id = channels.id
WHERE
    channels.guild_id = (SELECT id FROM guilds WHERE guild = ?) AND
    channels.channel = ? AND
    FIND_IN_SET(reminders.enabled, ?)
LIMIT
    ?
            ", guild_id, channel_id, enabled, flags.limit)
            .fetch_all(&pool)
            .await
    }
    else {
        sqlx::query_as!(Reminder,
            "
SELECT
    id, time, name, channel_id
FROM
    reminders
WHERE
    reminders.channel_id = (SELECT id FROM channels WHERE channel = ?) AND
    FIND_IN_SET(reminders.enabled, ?)
LIMIT
    ?
            ", msg.channel_id.as_u64(), enabled, flags.limit)
            .fetch_all(&pool)
            .await
    }.unwrap();

    if reminders.is_empty() {
        let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "look/no_reminders").await).await;
    }
    else {
        let inter = user_data.response(&pool, "look/inter").await;

        let display = reminders
            .iter()
            .map(|reminder| format!("'{}' *{}* **{}**", reminder.name, &inter, flags.display_time(reminder.time as u64)));

        let _ = msg.channel_id.say_lines(&ctx, display).await;
    }

    Ok(())
}

#[command]
#[permission_level(Managed)]
async fn delete(ctx: &Context, msg: &Message, _args: String) -> CommandResult {
    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let user_data = UserData::from_user(&msg.author, &ctx, &pool).await.unwrap();

    let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "del/listing").await).await;

    let reminders = if let Some(guild_id) = msg.guild_id.map(|f| f.as_u64().to_owned()) {
        sqlx::query_as!(Reminder,
            "
SELECT
    reminders.id, reminders.time, reminders.name, reminders.channel_id
FROM
    reminders
INNER JOIN
    channels
ON
    reminders.channel_id = channels.id
WHERE
    channels.guild_id = (SELECT id FROM guilds WHERE guild = ?)
            ", guild_id)
            .fetch_all(&pool)
            .await
    }
    else {
        sqlx::query_as!(Reminder,
            "
SELECT
    id, time, name, channel_id
FROM
    reminders
WHERE
    channel_id = (SELECT id FROM channels WHERE channel = ?)
            ", msg.channel_id.as_u64())
            .fetch_all(&pool)
            .await
    }.unwrap();

    let mut reminder_ids: Vec<u32> = vec![];

    let enumerated_reminders = reminders
        .iter()
        .enumerate()
        .map(|(count, reminder)| {
            reminder_ids.push(reminder.id);
            format!("**{}**: '{}' *{}* at {}", count + 1, reminder.name, reminder.channel_id, reminder.time)
        });

    let _ = msg.channel_id.say_lines(&ctx, enumerated_reminders).await;
    let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "del/listed").await).await;

    let reply = msg.channel_id.await_reply(&ctx)
        .filter_limit(1)
        .author_id(msg.author.id)
        .channel_id(msg.channel_id).await;

    if let Some(content) = reply.map(|m| m.content.replace(",", " ")) {
        let parts = content.split(' ').filter(|i| !i.is_empty()).collect::<Vec<&str>>();

        let valid_parts = parts
            .iter()
            .filter_map(
                |i|
                    i.parse::<usize>()
                        .ok()
                        .map(
                            |val|
                                reminder_ids.get(val - 1)
                        )
                        .flatten()
            )
            .map(|item| item.to_string())
            .collect::<Vec<String>>();

        if parts.len() == valid_parts.len() {
            sqlx::query!(
                "
DELETE FROM reminders WHERE FIND_IN_SET(id, ?)
                ", valid_parts.join(","))
                .execute(&pool)
                .await
                .unwrap();

            // TODO add deletion events to event list

            let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "del/count").await).await;
        }
    }

    Ok(())
}

#[command]
#[permission_level(Managed)]
async fn timer(ctx: &Context, msg: &Message, args: String) -> CommandResult {
        fn time_difference(start_time: NaiveDateTime) -> String {
        let unix_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        let now = NaiveDateTime::from_timestamp(unix_time, 0);

        let delta = (now - start_time).num_seconds();

        let (minutes, seconds) = delta.div_rem(&60);
        let (hours, minutes) = minutes.div_rem(&60);

        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    }

    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let user_data = UserData::from_user(&msg.author, &ctx, &pool).await.unwrap();

    let mut args_iter = args.splitn(2, ' ');

    let owner = msg.guild_id.map(|g| g.as_u64().to_owned()).unwrap_or_else(|| msg.author.id.as_u64().to_owned());

    match args_iter.next() {
        Some("list") => {
            let timers = Timer::from_owner(owner, &pool).await;

            let _ = msg.channel_id.send_message(&ctx, |m| m
                .embed(|e| {
                    e.fields(timers.iter().map(|timer| (&timer.name, time_difference(timer.start_time), false)))
                })
            ).await;
        },

        Some("start") => {
            let count = Timer::count_from_owner(owner, &pool).await;

            if count >= 25 {
                let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "timer/limit").await).await;
            }
            else {
                let name = args_iter.next().map(|s| s.to_string()).unwrap_or(format!("New timer #{}", count + 1));

                Timer::create(&name, owner, &pool).await;

                let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "timer/success").await).await;
            }
        },

        Some("delete") => {
            if let Some(name) = args_iter.next() {
                let exists = sqlx::query!(
                    "
SELECT 1 as _r FROM timers WHERE owner = ? AND name = ?
                    ", owner, name)
                    .fetch_one(&pool)
                    .await;

                if exists.is_ok() {
                    sqlx::query!(
                        "
DELETE FROM timers WHERE owner = ? AND name = ?
                        ", owner, name)
                        .execute(&pool)
                        .await
                        .unwrap();

                    let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "timer/deleted").await).await;
                }
                else {
                    let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "timer/not_found").await).await;
                }
            }
            else {
                let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "timer/help").await).await;
            }
        },

        _ => {
            let _ = msg.channel_id.say(&ctx, user_data.response(&pool, "timer/help").await).await;
        }
    }

    Ok(())
}

#[derive(PartialEq)]
enum RemindCommand {
    Remind,
    Interval,
}

enum ReminderScope {
    User(u64),
    Channel(u64),
}

impl Mentionable for ReminderScope {
    fn mention(&self) -> String {
        match self {
            Self::User(id) => format!("<@{}>", id),
            Self::Channel(id) => format!("<#{}>", id),
        }
    }
}

custom_error!{ReminderError
    LongTime = "Time too long",
    LongInterval = "Interval too long",
    PastTime = "Time has already passed",
    ShortInterval = "Interval too short",
    InvalidTag = "Invalid reminder scope",
    NotEnoughArgs = "Not enough args",
    InvalidTime = "Invalid time provided",
    DiscordError = "Bad response received from Discord"
}

impl ReminderError {
    fn to_response(&self) -> String {
        match self {
            Self::LongTime => "remind/long_time",
            Self::LongInterval => "interval/long_interval",
            Self::PastTime => "remind/past_time",
            Self::ShortInterval => "interval/short_interval",
            Self::InvalidTag => "remind/invalid_tag",
            Self::NotEnoughArgs => "remind/no_argument",
            Self::InvalidTime => "remind/invalid_time",
            Self::DiscordError => "remind/no_webhook",
        }.to_string()
    }
}

fn generate_uid() -> String {
    let mut generator: OsRng = Default::default();

    let mut bytes = vec![0u8, 64];

    generator.fill_bytes(&mut bytes);

    bytes.iter().map(|i| (CHARACTERS.as_bytes()[(i.to_owned() as usize) % CHARACTERS.len()] as char).to_string()).collect::<Vec<String>>().join("")
}

#[command]
#[permission_level(Managed)]
async fn remind(ctx: &Context, msg: &Message, args: String) -> CommandResult {
    remind_command(ctx, msg, args, RemindCommand::Remind).await;

    Ok(())
}

#[command]
#[permission_level(Managed)]
async fn interval(ctx: &Context, msg: &Message, args: String) -> CommandResult {
    remind_command(ctx, msg, args, RemindCommand::Interval).await;

    Ok(())
}

async fn remind_command(ctx: &Context, msg: &Message, args: String, command: RemindCommand) {

    async fn check_interval(
        ctx: impl CacheHttp + AsRef<Cache>,
        msg: &Message,
        mut args_iter: impl Iterator<Item=&str>,
        scope_id: &ReminderScope,
        time_parser: &TimeParser,
        command: RemindCommand,
        pool: &MySqlPool)
        -> Result<(), ReminderError> {

        let subscribed = check_subscription(&ctx, &msg.author).await ||
            if let Some(guild) = msg.guild(&ctx).await { check_subscription(&ctx, guild.owner_id).await } else { false };

        if command == RemindCommand::Interval && subscribed {
            if let Some(interval_arg) = args_iter.next() {
                let interval = TimeParser::new(interval_arg.to_string(), UTC);

                if let Ok(interval_seconds) = interval.displacement() {
                    let content = args_iter.collect::<Vec<&str>>().join(" ");

                    create_reminder(
                        ctx,
                        pool,
                        msg.author.id.as_u64().to_owned(),
                        msg.guild_id,
                        scope_id,
                        time_parser,
                        Some(interval_seconds),
                        content).await
                }
                else {
                    Err(ReminderError::InvalidTime)
                }
            }
            else {
                Err(ReminderError::NotEnoughArgs)
            }
        }
        else {
            let content = args_iter.collect::<Vec<&str>>().join(" ");

            create_reminder(
                ctx,
                pool,
                msg.author.id.as_u64().to_owned(),
                msg.guild_id,
                scope_id,
                time_parser,
                None,
                content).await
        }
    }

    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let user_data = UserData::from_user(&msg.author, &ctx, &pool).await.unwrap();

    let mut args_iter = args.split(' ').filter(|s| !s.is_empty());

    let mut time_parser = None;
    let mut scope_id = ReminderScope::Channel(msg.channel_id.as_u64().to_owned());

    // todo reimplement using next_if and Peekable
    let response = if let Some(first_arg) = args_iter.next().map(|s| s.to_string()) {
        if let Some((Some(scope_match), Some(id_match))) = REGEX_CHANNEL_USER
            .captures(&first_arg)
            .map(|cap| (cap.get(1), cap.get(2))) {

            if scope_match.as_str() == "@" {
                scope_id = ReminderScope::User(id_match.as_str().parse::<u64>().unwrap());
            }
            else {
                scope_id = ReminderScope::Channel(id_match.as_str().parse::<u64>().unwrap());
            }

            if let Some(next) = args_iter.next().map(|inner| inner.to_string()) {
                time_parser = Some(TimeParser::new(next, user_data.timezone.parse().unwrap()));

                check_interval(&ctx, msg, args_iter, &scope_id, &time_parser.as_ref().unwrap(), command, &pool).await
            }
            else {
                Err(ReminderError::NotEnoughArgs)
            }
        }
        else {
            time_parser = Some(TimeParser::new(first_arg, user_data.timezone()));

            check_interval(&ctx, msg, args_iter, &scope_id, &time_parser.as_ref().unwrap(), command, &pool).await
        }
    }
    else {
        Err(ReminderError::NotEnoughArgs)
    };

    let str_response = match response {
        Ok(_) => user_data.response(&pool, "remind/success").await,

        Err(reminder_error) => user_data.response(&pool, &reminder_error.to_response()).await,
    }
        .replacen("{location}", &scope_id.mention(), 1)
        .replacen("{offset}", &time_parser.map(|tp| tp.displacement().ok()).flatten().unwrap_or(-1).to_string(), 1)
        .replacen("{min_interval}", &MIN_INTERVAL.to_string(), 1)
        .replacen("{max_time}", &MAX_TIME.to_string(), 1);

    let _ = msg.channel_id.say(&ctx, &str_response).await;
}

#[command]
async fn natural(ctx: &Context, msg: &Message, args: String) -> CommandResult {
    let pool = ctx.data.read().await
        .get::<SQLPool>().cloned().expect("Could not get SQLPool from data");

    let user_data = UserData::from_user(&msg.author, &ctx, &pool).await.unwrap();

    let send_str = user_data.response(&pool, "natural/send").await;
    let to_str = user_data.response(&pool, "natural/to").await;
    let every_str = user_data.response(&pool, "natural/every").await;

    let mut args_iter = args.splitn(2, &send_str);

    let (time_crop_opt, msg_crop_opt) = (args_iter.next(), args_iter.next());

    if let (Some(time_crop), Some(msg_crop)) = (time_crop_opt, msg_crop_opt) {
        let python_call = Command::new("venv/bin/python3")
            .arg("dp.py")
            .arg(time_crop)
            .arg(user_data.timezone)
            .arg(&env::var("LOCAL_TIMEZONE").unwrap_or_else(|_| "UTC".to_string()))
            .output()
            .await;

        if let Some(timestamp) = python_call.ok().map(|inner|
            if inner.status.success() {
                Some(from_utf8(&*inner.stdout).unwrap().parse::<i64>().unwrap())
            }
            else {
                None
            }).flatten() {

            let mut location_ids = vec![ReminderScope::Channel(msg.channel_id.as_u64().to_owned())];
            let mut content = msg_crop;
            let mut interval = None;

            // check other options and then create reminder :)
            if msg.guild_id.is_some() {
                let re_match = Regex::new(&format!(r#"(?P<msg>.*) {} (?P<mentions>((?:<@\d+>)|(?:<@!\d+>)|(?:<#\d+>)|(?:\s+))+)$"#, to_str))
                    .unwrap()
                    .captures(msg_crop);

                if let Some(captures) = re_match {
                    content = captures.name("msg").unwrap().as_str();

                    let mentions = captures.name("mentions").unwrap().as_str();
                    location_ids = REGEX_CHANNEL_USER
                        .captures_iter(mentions)
                        .map(|i| {
                            let pref = i.get(1).unwrap().as_str();
                            let id = i.get(2).unwrap().as_str().parse::<u64>().unwrap();

                            if pref == "#" {
                                ReminderScope::Channel(id)
                            } else {
                                ReminderScope::User(id)
                            }
                        }).collect::<Vec<ReminderScope>>();
                }
            }

            let subscribed = check_subscription(&ctx, &msg.author).await ||
                if let Some(guild) = msg.guild(&ctx).await { check_subscription(&ctx, guild.owner_id).await } else { false };

            if subscribed {
                let re_match = Regex::new(&format!(r#"(?P<msg>.*) {} (?P<interval>.*)$"#, every_str))
                    .unwrap()
                    .captures(content);

                if let Some(captures) = re_match {
                    content = captures.name("msg").unwrap().as_str();

                    let interval_str = captures.name("interval").unwrap().as_str();

                    let python_call = Command::new("venv/bin/python3")
                        .arg("dp.py")
                        .arg(&format!("1 {}", interval_str))
                        .arg(&env::var("LOCAL_TIMEZONE").unwrap_or_else(|_| "UTC".to_string()))
                        .arg(&env::var("LOCAL_TIMEZONE").unwrap_or_else(|_| "UTC".to_string()))
                        .output()
                        .await;

                    let now = SystemTime::now();
                    let since_epoch = now
                        .duration_since(UNIX_EPOCH)
                        .expect("Time calculated as going backwards. Very bad");

                    interval = python_call.ok().map(|inner|
                        if inner.status.success() {
                            Some(from_utf8(&*inner.stdout).unwrap().parse::<i64>().unwrap() - since_epoch.as_secs() as i64)
                        }
                        else {
                            None
                        }).flatten();
                }

            }

            let mut issue_count = 0;

            for location in location_ids {
                let res = create_reminder(
                    &ctx,
                    &pool,
                    msg.author.id.as_u64().to_owned(),
                    msg.guild_id,
                    &location,
                    timestamp,
                    interval,
                    &content).await;

                if res.is_ok() {
                    issue_count += 1;
                }
            }

            let _ = msg.channel_id.say(&ctx, format!("successfully set {} reminders", issue_count)).await;
        }
        else {
            let _ = msg.channel_id.say(&ctx, "DEV ERROR: Failed to invoke Python").await;
        }
    }
    else {
        let prefix = GuildData::prefix_from_id(msg.guild_id, &pool).await;

        let resp = user_data.response(&pool, "natural/no_argument").await.replace("{prefix}", &prefix);

        let _ = msg.channel_id.send_message(&ctx, |m| m
            .embed(|e| e
                .description(resp)
            )).await;
    }

    Ok(())
}

async fn create_reminder<T: TryInto<i64>, S: ToString + Type<MySql> + Encode<MySql>>(
    ctx: impl CacheHttp,
    pool: &MySqlPool,
    user_id: u64,
    guild_id: Option<GuildId>,
    scope_id: &ReminderScope,
    time_parser: T,
    interval: Option<i64>,
    content: S)
    -> Result<(), ReminderError> {

    let content_string = content.to_string();

    let db_channel_id = match scope_id {
        ReminderScope::User(user_id) => {
            let user = UserId(*user_id).to_user(&ctx).await.unwrap();

            let user_data = UserData::from_user(&user, &ctx, &pool).await.unwrap();

            user_data.dm_channel
        },

        ReminderScope::Channel(channel_id) => {
            let channel = ChannelId(*channel_id).to_channel(&ctx).await.unwrap();

            if channel.clone().guild().map(|gc| gc.guild_id) != guild_id {
                return Err(ReminderError::InvalidTag)
            }

            let mut channel_data = ChannelData::from_channel(channel.clone(), &pool).await.unwrap();

            if let Some(guild_channel) = channel.guild() {
                if channel_data.webhook_token.is_none() || channel_data.webhook_id.is_none() {
                    if let Ok(webhook) = ctx.http().create_webhook(guild_channel.id.as_u64().to_owned(), &json!({"name": "Reminder"})).await {
                        channel_data.webhook_id = Some(webhook.id.as_u64().to_owned());
                        channel_data.webhook_token = Some(webhook.token);

                        channel_data.commit_changes(&pool).await;
                    }
                    else {
                        return Err(ReminderError::DiscordError)
                    }
                }
            }

            channel_data.id
        },
    };

    // validate time, channel, content
    if content_string.is_empty() {
        Err(ReminderError::NotEnoughArgs)
    }
    else if interval.map_or(false, |inner| inner < *MIN_INTERVAL) {
        Err(ReminderError::ShortInterval)
    }
    else if interval.map_or(false, |inner| inner > *MAX_TIME) {
        Err(ReminderError::LongInterval)
    }
    else {
        match time_parser.try_into() {
            Ok(time) => {
                let unix_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;

                if time > unix_time {
                    if time > unix_time + *MAX_TIME {
                        Err(ReminderError::LongTime)
                    }
                    else {
                        sqlx::query!(
                            "
INSERT INTO messages (content) VALUES (?)
                            ", content)
                            .execute(&pool.clone())
                            .await
                            .unwrap();

                        sqlx::query!(
                            "
INSERT INTO reminders (uid, message_id, channel_id, time, `interval`, method, set_by) VALUES
    (?,
    (SELECT id FROM messages WHERE content = ? ORDER BY id DESC LIMIT 1),
    ?, ?, ?, 'remind',
    (SELECT id FROM users WHERE user = ? LIMIT 1))
                            ", generate_uid(), content, db_channel_id, time as u32, interval, user_id)
                            .execute(pool)
                            .await
                            .unwrap();

                        Ok(())
                    }
                }
                else {
                    Err(ReminderError::PastTime)
                }
            },

            Err(_) => {
                Err(ReminderError::InvalidTime)
            },
        }
    }
}
