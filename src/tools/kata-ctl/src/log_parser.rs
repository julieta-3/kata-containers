// Copyright (c) 2023 Gabe Venberg
//
// SPDX-License-Identifier: Apache-2.0

#![warn(missing_debug_implementations)]

mod log_message;
mod log_parser_error;
mod output_file;
mod parse_file;
mod process_logs;
mod structured_parser;

use crate::args::LogParser;
use anyhow::Context;
use log_message::AnyLogMessage;
use log_message::LogMessage;
use log_message::StrictLogMessage;
use log_parser_error::LogParserError;
use output_file::*;
use parse_file::*;
use process_logs::*;

fn handle_logs_impl<T: AnyLogMessage>(mut cli: LogParser, apply_struct: fn(&mut [T]) -> ()) -> Result<(), LogParserError> {
    let mut logs = Vec::new();
    let should_parse_structured = cli.try_parse_structured;

    for file in &cli.input_file {
        let in_file = open_file_into_memory(file)?;
        let file_logs = filter_errors(parse_log::<T>(in_file), &cli)?;

        if cli.error_if_file_empty && file_logs.is_empty() {
            return Err(LogParserError::FileEmpty(file.to_path_buf()));
        }

        logs.extend(file_logs)
    }

    if cli.error_if_no_records && logs.is_empty() {
        return Err(LogParserError::NoRecordsError());
    }
    if cli.check_only {
        return Ok(());
    }

    sort_logs(&mut logs);
    
    // Apply structured data parsing if requested
    if should_parse_structured {
        apply_struct(&mut logs);
    }
    
    output_file(logs, &cli)?;
    Ok(())
}

fn noop_apply_structured(_logs: &mut [LogMessage]) {}
fn apply_structured_to_log_message(logs: &mut [LogMessage]) {
    for log in logs {
        let (_prefix, structured) = structured_parser::try_parse_structured(&log.message);
        if let Some(structured_data) = structured {
            log.msg_struct = Some(structured_data);
        }
    }
}

fn noop_apply_structured_strict(_logs: &mut [StrictLogMessage]) {}
fn apply_structured_to_strict_log_message(logs: &mut [StrictLogMessage]) {
    for log in logs {
        let (_prefix, structured) = structured_parser::try_parse_structured(&log.message);
        if let Some(structured_data) = structured {
            log.msg_struct = Some(structured_data);
        }
    }
}

//needed another layer of function call in order to genericize over both LogMessage and
//StrictLogMessage.
pub fn log_parser(args: LogParser) -> anyhow::Result<()> {
    if args.ignore_missing_fields {
        handle_logs_impl::<LogMessage>(args, apply_structured_to_log_message)
    } else {
        handle_logs_impl::<StrictLogMessage>(args, apply_structured_to_strict_log_message)
    }
    .context("Could not parse logs")
}
