// Copyright (c) 2023 Gabe Venberg
//
// SPDX-License-Identifier: Apache-2.0

use serde_json::{json, Value as JsonValue};

// Attempts to parse structured data from a message string using multiple approaches.
//
// This function tries to parse Rust debug format data that may be embedded in log messages.
// It employs a multi-stage pipeline:
// -Try JSON parsing (for actual JSON)
// -Try RON parsing (Rusty Object Notation)
// -Try Rust debug format heuristics
// -Fall back to None
//
// Returns (human_readable_prefix, parsed_json_value)
pub fn try_parse_structured(msg: &str) -> (String, Option<JsonValue>) {
    // Try to find and extract structured data

    // Try JSON first
    if let Ok(json_val) = serde_json::from_str::<JsonValue>(msg) {
        return (String::new(), Some(json_val));
    }
    
    // Try to extract and parse RON
    if let Some((prefix, structured_part)) = extract_structured_part(msg) {
        if let Ok(ron_val) = ron::from_str::<ron::Value>(&structured_part) {
            if let Ok(json_val) = ron_to_json(&ron_val) {
                return (prefix, Some(json_val));
            }
        }
        
        // Try to parse as debug format by converting to JSON 
        if let Some(json_val) = parse_debug_format(&structured_part) {
            return (prefix, Some(json_val));
        }
    }
    
    // No structured data found
    (msg.to_string(), None)
}

// Extracts the human readable prefix and structured data part from a message.
// Returns prefix, structured_part or None if no structured data found.
fn extract_structured_part(msg: &str) -> Option<(String, String)> {
    for (i, c) in msg.char_indices() {
        if (c == '{' || c == '[') && is_at_word_boundary(msg, i) {
            // Check if we can find matching closing bracket
            let closing = if c == '{' { '}' } else { ']' };
            if let Some(end) = find_matching_bracket(msg, i, c, closing) {
                let prefix = msg[..i].to_string();
                let structured = msg[i..=end].to_string();
                return Some((prefix, structured));
            }
        }
    }
    
    None
}

// Checks if the given position is at a boundary
fn is_at_word_boundary(msg: &str, pos: usize) -> bool {
    if pos == 0 {
        return true;
    }
    
    let before = &msg[..pos];
    if let Some(last_char) = before.chars().last() {
        last_char.is_whitespace() || last_char == ':' || last_char == '(' || last_char == '['
    } else {
        true
    }
}

// Finds the index of the matching closing bracket.
fn find_matching_bracket(msg: &str, start: usize, open_char: char, close_char: char) -> Option<usize> {
    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;
    
    for (i, c) in msg[start..].char_indices() {
        let actual_idx = start + i;
        
        if escape_next {
            escape_next = false;
            continue;
        }
        
        if c == '\\' {
            escape_next = true;
            continue;
        }
        
        if c == '"' && !in_string {
            in_string = true;
            continue;
        }
        
        if c == '"' && in_string {
            in_string = false;
            continue;
        }
        
        if in_string {
            continue;
        }
        
        if c == open_char {
            depth += 1;
        } else if c == close_char {
            depth -= 1;
            if depth == 0 {
                return Some(actual_idx);
            }
        }
    }
    
    None
}

// Converts RON Value to JSON Value.
fn ron_to_json(ron_val: &ron::Value) -> Result<JsonValue, Box<dyn std::error::Error>> {
    match ron_val {
        ron::Value::Bool(b) => Ok(JsonValue::Bool(*b)),
        ron::Value::Char(c) => Ok(JsonValue::String(c.to_string())),
        ron::Value::Map(map) => {
            let mut obj = serde_json::Map::new();
            for (key, val) in map.iter() {
                let key_str = match key {
                    ron::Value::String(s) => s.clone(),
                    other => ron::to_string(other)?,
                };
                obj.insert(key_str, ron_to_json(val)?);
            }
            Ok(JsonValue::Object(obj))
        }
        ron::Value::Number(num) => {
            let num_str = num.to_string();
            if num_str.contains('.') {
                Ok(JsonValue::Number(
                    serde_json::Number::from_f64(num_str.parse::<f64>()?)
                        .ok_or("Invalid number")?,
                ))
            } else {
                Ok(JsonValue::Number(
                    serde_json::Number::from_f64(num_str.parse::<i64>()? as f64)
                        .ok_or("Invalid number")?,
                ))
            }
        }
        ron::Value::String(s) => Ok(JsonValue::String(s.clone())),
        ron::Value::Seq(seq) => {
            let items = seq
                .iter()
                .map(ron_to_json)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(JsonValue::Array(items))
        }
        ron::Value::Unit => Ok(JsonValue::Null),
        ron::Value::Option(opt) => match opt {
            Some(inner) => ron_to_json(inner),
            None => Ok(JsonValue::Null),
        },
    }
}

// Attempts to parse Rust debug format into a JSON-like structure.
fn parse_debug_format(data: &str) -> Option<JsonValue> {
    let trimmed = data.trim();
    
    
    if let Some(stripped) = trimmed.strip_prefix('{') {
        if let Some(inner) = stripped.strip_suffix('}') {
            return parse_debug_object(inner);
        }
    }
    
    if let Some(stripped) = trimmed.strip_prefix('[') {
        if let Some(inner) = stripped.strip_suffix(']') {
            return parse_debug_array(inner);
        }
    }
    
    if let Some(stripped) = trimmed.strip_prefix('(') {
        if let Some(inner) = stripped.strip_suffix(')') {
            return parse_debug_array(inner);
        }
    }
    
    // Try to parse as a simple type name with parentheses
    if let Some(paren_idx) = trimmed.find('(') {
        let type_name = trimmed[..paren_idx].trim();
        if let Some(content) = trimmed[paren_idx..].strip_prefix('(') {
            if let Some(inner) = content.strip_suffix(')') {
                // Store the type name and content
                let mut obj = serde_json::Map::new();
                obj.insert("_type".to_string(), JsonValue::String(type_name.to_string()));
                if !inner.is_empty() {
                    obj.insert("_value".to_string(), JsonValue::String(inner.to_string()));
                }
                return Some(JsonValue::Object(obj));
            }
        }
    }
    
    None
}

// Parses the contents of a debug format object (between { })
fn parse_debug_object(content: &str) -> Option<JsonValue> {
    let mut obj = serde_json::Map::new();
    
    let pairs = split_debug_pairs(content);
    for pair in pairs {
        if let Some((key, value)) = parse_debug_pair(&pair) {
            obj.insert(key, value);
        }
    }
    
    if obj.is_empty() {
        None
    } else {
        Some(JsonValue::Object(obj))
    }
}

// Parses the contents of a debug format array (between [] or ())
fn parse_debug_array(content: &str) -> Option<JsonValue> {
    if content.trim().is_empty() {
        return Some(JsonValue::Array(Vec::new()));
    }
    
    let items = split_debug_items(content);
    let json_items = items
        .into_iter()
        .map(|item| {
            // Try to parse as various types
            if let Ok(n) = item.parse::<i64>() {
                JsonValue::Number(serde_json::Number::from(n))
            } else if let Ok(n) = item.parse::<f64>() {
                JsonValue::Number(
                    serde_json::Number::from_f64(n).unwrap_or(serde_json::Number::from(0)),
                )
            } else if item == "true" {
                JsonValue::Bool(true)
            } else if item == "false" {
                JsonValue::Bool(false)
            } else if item == "None" {
                JsonValue::Null
            } else {
                JsonValue::String(item)
            }
        })
        .collect();
    
    Some(JsonValue::Array(json_items))
}

// Splits content by commas
fn split_debug_pairs(content: &str) -> Vec<String> {
    let mut pairs = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;
    
    for c in content.chars() {
        if escape_next {
            current.push(c);
            escape_next = false;
            continue;
        }
        
        if c == '\\' {
            current.push(c);
            escape_next = true;
            continue;
        }
        
        if c == '"' {
            in_string = !in_string;
            current.push(c);
            continue;
        }
        
        if in_string {
            current.push(c);
            continue;
        }
        
        match c {
            '{' | '[' | '(' => {
                depth += 1;
                current.push(c);
            }
            '}' | ']' | ')' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                if !current.trim().is_empty() {
                    pairs.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }
    
    if !current.trim().is_empty() {
        pairs.push(current.trim().to_string());
    }
    
    pairs
}

// Splits items by commas, respecting nested structures
fn split_debug_items(content: &str) -> Vec<String> {
    split_debug_pairs(content)
}

// Parses a key: value pair
fn parse_debug_pair(pair: &str) -> Option<(String, JsonValue)> {
    // Look for the first colon that's not in a nested structure
    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;
    
    for (i, c) in pair.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        
        if c == '\\' {
            escape_next = true;
            continue;
        }
        
        if c == '"' {
            in_string = !in_string;
            continue;
        }
        
        if in_string {
            continue;
        }
        
        match c {
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth -= 1,
            ':' if depth == 0 => {
                let key = pair[..i].trim().to_string();
                let value_str = pair[i + 1..].trim();
                let value = parse_debug_value(value_str);
                return Some((key, value));
            }
            _ => {}
        }
    }
    
    None
}

// Parses a value in debug format
fn parse_debug_value(value_str: &str) -> JsonValue {
    let trimmed = value_str.trim();
    
    // Try to parse as number
    if let Ok(n) = trimmed.parse::<i64>() {
        return JsonValue::Number(serde_json::Number::from(n));
    }
    
    if let Ok(n) = trimmed.parse::<f64>() {
        if let Some(num) = serde_json::Number::from_f64(n) {
            return JsonValue::Number(num);
        }
    }
    
    // Try to parse as boolean
    if trimmed == "true" {
        return JsonValue::Bool(true);
    }
    
    if trimmed == "false" {
        return JsonValue::Bool(false);
    }
    
    // Try to parse as null
    if trimmed == "None" || trimmed == "()" {
        return JsonValue::Null;
    }
    
    // Try to parse as nested structure
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
        || (trimmed.starts_with('(') && trimmed.ends_with(')'))
    {
        if let Some(parsed) = parse_debug_format(trimmed) {
            return parsed;
        }
    }
    
    // Try to remove quotes 
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        return JsonValue::String(trimmed[1..trimmed.len() - 1].to_string());
    }
    
    // Default to string
    JsonValue::String(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_parsing() {
        let (prefix, parsed) = try_parse_structured(r#"{"key": "value"}"#);
        assert!(parsed.is_some());
        assert_eq!(prefix, "");
    }

    #[test]
    fn test_simple_debug_format() {
        let (prefix, parsed) = try_parse_structured("some prefix: { field1: value1, field2: 42 }");
        assert!(parsed.is_some());
        assert!(!prefix.is_empty());
        let obj = parsed.unwrap().as_object().unwrap();
        assert_eq!(obj.get("field1").unwrap(), "value1");
    }

    #[test]
    fn test_debug_format_with_nested_structures() {
        let (prefix, parsed) = try_parse_structured("DeviceResources([{base: 1234, size: 5678}])");
        assert!(parsed.is_some());
    }

    #[test]
    fn test_no_structured_data() {
        let (prefix, parsed) = try_parse_structured("just a simple message");
        assert!(parsed.is_none());
        assert_eq!(prefix, "just a simple message");
    }
}
