/// CBOR validation implementation
pub mod cbor;

/// JSON validation implementation
pub mod json;

use std::error::Error;

use cbor::CBORValidator;
use json::JSONValidator;
use serde::de::Deserialize;

use crate::{
  ast::{
    GroupChoice, GroupEntry, GroupRule, Identifier, Occur, Rule, Type, Type2, TypeChoice, TypeRule,
    CDDL,
  },
  cddl_from_str, lexer_from_str,
  token::*,
  visitor::Visitor,
};

trait Validator<'a, E: Error>: Visitor<'a, E> {
  fn validate(&mut self) -> std::result::Result<(), E>;
  fn add_error(&mut self, reason: String);
}

trait Validatable {}

impl Validatable for serde_cbor::Value {}
impl Validatable for serde_json::Value {}

/// Validate JSON string from a given CDDL document string
pub fn validate_json_from_str(cddl: &str, json: &str) -> json::Result {
  let mut lexer = lexer_from_str(cddl);
  let cddl = cddl_from_str(&mut lexer, cddl, true).map_err(json::Error::CDDLParsing)?;
  let json = serde_json::from_str::<serde_json::Value>(json).map_err(json::Error::JSONParsing)?;

  let mut jv = JSONValidator::new(&cddl, json);
  jv.validate()
}

/// Validate CBOR slice from a given CDDL document string
pub fn validate_cbor_from_slice(cddl: &str, cbor_slice: &[u8]) -> cbor::Result {
  let mut lexer = lexer_from_str(cddl);
  let cddl = cddl_from_str(&mut lexer, cddl, true).map_err(cbor::Error::CDDLParsing)?;
  let cbor =
    serde_cbor::from_slice::<serde_cbor::Value>(cbor_slice).map_err(cbor::Error::CBORParsing)?;

  let mut cv = CBORValidator::new(&cddl, cbor);
  cv.validate()
}

/// Find non-choice alternate rule from a given identifier
pub fn rule_from_ident<'a>(cddl: &'a CDDL, ident: &Identifier) -> Option<&'a Rule<'a>> {
  cddl.rules.iter().find_map(|r| match r {
    Rule::Type { rule, .. } if rule.name == *ident && !rule.is_type_choice_alternate => Some(r),
    Rule::Group { rule, .. } if rule.name == *ident && !rule.is_group_choice_alternate => Some(r),
    _ => None,
  })
}

/// Unwrap array, map or tag type rule from ident
pub fn unwrap_rule_from_ident<'a>(cddl: &'a CDDL, ident: &Identifier) -> Option<&'a Rule<'a>> {
  cddl.rules.iter().find_map(|r| match r {
    Rule::Type {
      rule:
        TypeRule {
          name,
          is_type_choice_alternate,
          value: Type { type_choices, .. },
          ..
        },
      ..
    } if name == ident && !is_type_choice_alternate => {
      let match_fn = |tc: &TypeChoice| {
        matches!(
          tc.type1.type2,
          Type2::Map { .. } | Type2::Array { .. } | Type2::TaggedData { .. }
        )
      };

      if type_choices.iter().any(match_fn) {
        Some(r)
      } else if let Some(ident) = type_choices.iter().find_map(|tc| {
        if let Type2::Typename {
          ident,
          generic_args: None,
          ..
        } = &tc.type1.type2
        {
          Some(ident)
        } else {
          None
        }
      }) {
        unwrap_rule_from_ident(cddl, ident)
      } else {
        None
      }
    }
    _ => None,
  })
}

/// Find non-group choice alternate rule from a given identifier
pub fn group_rule_from_ident<'a>(cddl: &'a CDDL, ident: &Identifier) -> Option<&'a GroupRule<'a>> {
  cddl.rules.iter().find_map(|r| match r {
    Rule::Group { rule, .. } if rule.name == *ident && !rule.is_group_choice_alternate => {
      Some(rule.as_ref())
    }
    _ => None,
  })
}

/// Find non-group choice alternate rule from a given identifier
pub fn type_rule_from_ident<'a>(cddl: &'a CDDL, ident: &Identifier) -> Option<&'a TypeRule<'a>> {
  cddl.rules.iter().find_map(|r| match r {
    Rule::Type { rule, .. } if rule.name == *ident && !rule.is_type_choice_alternate => Some(rule),
    _ => None,
  })
}

/// Retrieve the list of generic parameters for a given rule
pub fn generic_params_from_rule<'a>(rule: &Rule<'a>) -> Option<Vec<&'a str>> {
  match rule {
    Rule::Type { rule, .. } => rule
      .generic_params
      .as_ref()
      .map(|gp| gp.params.iter().map(|gp| gp.param.ident).collect()),
    Rule::Group { rule, .. } => rule
      .generic_params
      .as_ref()
      .map(|gp| gp.params.iter().map(|gp| gp.param.ident).collect()),
  }
}

/// Find all type choice alternate rules from a given identifier
pub fn type_choice_alternates_from_ident<'a>(
  cddl: &'a CDDL,
  ident: &Identifier,
) -> Vec<&'a Type<'a>> {
  cddl
    .rules
    .iter()
    .filter_map(|r| match r {
      Rule::Type { rule, .. } if &rule.name == ident => Some(&rule.value),
      _ => None,
    })
    .collect::<Vec<_>>()
}

/// Find all group choice alternate rules from a given identifier
pub fn group_choice_alternates_from_ident<'a>(
  cddl: &'a CDDL,
  ident: &Identifier,
) -> Vec<&'a GroupEntry<'a>> {
  cddl
    .rules
    .iter()
    .filter_map(|r| match r {
      Rule::Group { rule, .. } if &rule.name == ident => Some(&rule.entry),
      _ => None,
    })
    .collect::<Vec<_>>()
}

/// Convert a given group choice to a list of type choices
pub fn type_choices_from_group_choice<'a>(
  cddl: &'a CDDL,
  grpchoice: &GroupChoice<'a>,
) -> Vec<TypeChoice<'a>> {
  let mut type_choices = Vec::new();
  for ge in grpchoice.group_entries.iter() {
    match &ge.0 {
      GroupEntry::ValueMemberKey { ge, .. } => {
        type_choices.append(&mut ge.entry_type.type_choices.clone());
      }
      GroupEntry::TypeGroupname { ge, .. } => {
        // TODO: parse generic args
        if let Some(r) = rule_from_ident(cddl, &ge.name) {
          match r {
            Rule::Type { rule, .. } => type_choices.append(&mut rule.value.type_choices.clone()),
            Rule::Group { rule, .. } => type_choices.append(&mut type_choices_from_group_choice(
              cddl,
              &GroupChoice::new(vec![rule.entry.clone()]),
            )),
          }
        }
      }
      GroupEntry::InlineGroup { group, .. } => {
        for gc in group.group_choices.iter() {
          type_choices.append(&mut type_choices_from_group_choice(cddl, gc));
        }
      }
    }
  }

  type_choices
}

/// Is the given identifier associated with a null data type
pub fn is_ident_null_data_type(cddl: &CDDL, ident: &Identifier) -> bool {
  if let Token::NULL | Token::NIL = lookup_ident(ident.ident) {
    return true;
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if &rule.name == ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        is_ident_null_data_type(cddl, ident)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Is the given identifier associated with a boolean data type
pub fn is_ident_bool_data_type(cddl: &CDDL, ident: &Identifier) -> bool {
  if let Token::BOOL = lookup_ident(ident.ident) {
    return true;
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if &rule.name == ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        is_ident_bool_data_type(cddl, ident)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Does the given boolean identifier match the boolean value
pub fn ident_matches_bool_value(cddl: &CDDL, ident: &Identifier, value: bool) -> bool {
  if let Token::TRUE = lookup_ident(ident.ident) {
    if value {
      return true;
    }
  }

  if let Token::FALSE = lookup_ident(ident.ident) {
    if !value {
      return true;
    }
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if &rule.name == ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        ident_matches_bool_value(cddl, ident, value)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Is the given identifier associated with a URI data type
pub fn is_ident_uri_data_type(cddl: &CDDL, ident: &Identifier) -> bool {
  if let Token::URI = lookup_ident(ident.ident) {
    return true;
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if &rule.name == ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        is_ident_uri_data_type(cddl, ident)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Is the given identifier associated with a b64url data type
pub fn is_ident_b64url_data_type(cddl: &CDDL, ident: &Identifier) -> bool {
  if let Token::B64URL = lookup_ident(ident.ident) {
    return true;
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if &rule.name == ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        is_ident_b64url_data_type(cddl, ident)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Is the given identifier associated with a tdate data type
pub fn is_ident_tdate_data_type(cddl: &CDDL, ident: &Identifier) -> bool {
  if let Token::TDATE = lookup_ident(ident.ident) {
    return true;
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if &rule.name == ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        is_ident_tdate_data_type(cddl, ident)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Is the given identifier associated with a time data type
pub fn is_ident_time_data_type(cddl: &CDDL, ident: &Identifier) -> bool {
  if let Token::TIME = lookup_ident(ident.ident) {
    return true;
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if &rule.name == ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        is_ident_time_data_type(cddl, ident)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Is the given identifier associated with a numeric data type
pub fn is_ident_numeric_data_type(cddl: &CDDL, ident: &Identifier) -> bool {
  if let Token::UINT
  | Token::NINT
  | Token::INTEGER
  | Token::INT
  | Token::NUMBER
  | Token::FLOAT
  | Token::FLOAT16
  | Token::FLOAT32
  | Token::FLOAT64
  | Token::FLOAT1632
  | Token::FLOAT3264
  | Token::UNSIGNED = lookup_ident(ident.ident)
  {
    return true;
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if rule.name == *ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        is_ident_numeric_data_type(cddl, ident)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Is the given identifier associated with a uint data type
pub fn is_ident_uint_data_type(cddl: &CDDL, ident: &Identifier) -> bool {
  if let Token::UINT = lookup_ident(ident.ident) {
    return true;
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if rule.name == *ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        is_ident_uint_data_type(cddl, ident)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Is the given identifier associated with a nint data type
pub fn is_ident_nint_data_type(cddl: &CDDL, ident: &Identifier) -> bool {
  if let Token::NINT = lookup_ident(ident.ident) {
    return true;
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if rule.name == *ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        is_ident_nint_data_type(cddl, ident)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Is the given identifier associated with an integer data type
pub fn is_ident_integer_data_type(cddl: &CDDL, ident: &Identifier) -> bool {
  if let Token::INT | Token::INTEGER | Token::NINT | Token::UINT | Token::NUMBER | Token::UNSIGNED =
    lookup_ident(ident.ident)
  {
    return true;
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if rule.name == *ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        is_ident_integer_data_type(cddl, ident)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Is the given identifier associated with a float data type
pub fn is_ident_float_data_type(cddl: &CDDL, ident: &Identifier) -> bool {
  if let Token::FLOAT
  | Token::FLOAT16
  | Token::FLOAT1632
  | Token::FLOAT32
  | Token::FLOAT3264
  | Token::FLOAT64 = lookup_ident(ident.ident)
  {
    return true;
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if rule.name == *ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        is_ident_float_data_type(cddl, ident)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Is the given identifier associated with a string data type
pub fn is_ident_string_data_type(cddl: &CDDL, ident: &Identifier) -> bool {
  if let Token::TEXT | Token::TSTR = lookup_ident(ident.ident) {
    return true;
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if rule.name == *ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        is_ident_string_data_type(cddl, ident)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Is the given identifier associated with the any type
pub fn is_ident_any_type(cddl: &CDDL, ident: &Identifier) -> bool {
  if let Token::ANY = lookup_ident(ident.ident) {
    return true;
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if rule.name == *ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        is_ident_any_type(cddl, ident)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Is the given identifier associated with a byte string data type
pub fn is_ident_byte_string_data_type(cddl: &CDDL, ident: &Identifier) -> bool {
  if let Token::BSTR | Token::BYTES = lookup_ident(ident.ident) {
    return true;
  }

  cddl.rules.iter().any(|r| match r {
    Rule::Type { rule, .. } if rule.name == *ident => rule.value.type_choices.iter().any(|tc| {
      if let Type2::Typename { ident, .. } = &tc.type1.type2 {
        is_ident_byte_string_data_type(cddl, ident)
      } else {
        false
      }
    }),
    _ => false,
  })
}

/// Retrieve all text strings and byte string literals from a given rule
/// identifier. Used for proposed .cat control operator.
pub fn string_literals_from_ident<'a>(cddl: &'a CDDL, ident: &Identifier) -> Vec<&'a Type2<'a>> {
  let mut literals = Vec::new();
  for r in cddl.rules.iter() {
    if let Rule::Type { rule, .. } = r {
      if rule.name == *ident {
        for tc in rule.value.type_choices.iter() {
          match &tc.type1.type2 {
            t @ Type2::TextValue { .. }
            | t @ Type2::UTF8ByteString { .. }
            | t @ Type2::B16ByteString { .. }
            | t @ Type2::B64ByteString { .. } => literals.push(t),
            Type2::Typename { ident, .. } => {
              literals.append(&mut string_literals_from_ident(cddl, ident))
            }
            _ => continue,
          }
          // if let Type2::TextValue { value, .. } = tc.type1.type2 {
          //   literals.push(value);
          // } else if let Type2::UTF8ByteString { value, .. } = &tc.type1.type2 {
          //   literals.push(std::str::from_utf8(&value).unwrap());
          // }
        }
      }
    }
  }

  literals
}

/// Validate array length and [non]homogeneity based on a given optional
/// occurrence indicator. The first bool in the returned tuple indicates whether
/// or not a subsequent validation of the array's elements shouch be homogenous.
/// The second bool in the returned tuple indicates whether or not an empty
/// array is allowed during a subsequent validation of the array's elements.
pub fn validate_array_occurrence<'de, T: Deserialize<'de>>(
  occurrence: Option<&Occur>,
  entry_counts: Option<&[EntryCount]>,
  values: &[T],
) -> std::result::Result<(bool, bool), Vec<String>> {
  let mut iter_items = false;
  let allow_empty_array = matches!(occurrence, Some(Occur::Optional(_)));

  let mut errors = Vec::new();

  match occurrence {
    Some(Occur::ZeroOrMore(_)) => iter_items = true,
    Some(Occur::OneOrMore(_)) => {
      if values.is_empty() {
        errors.push("array must have at least one item".to_string());
      } else {
        iter_items = true;
      }
    }
    Some(Occur::Exact { lower, upper, .. }) => {
      if let Some(lower) = lower {
        if let Some(upper) = upper {
          if lower == upper && values.len() != *lower {
            errors.push(format!("array must have exactly {} items", lower));
          }
          if values.len() < *lower || values.len() > *upper {
            errors.push(format!(
              "array must have between {} and {} items",
              lower, upper
            ));
          }
        } else if values.len() < *lower {
          errors.push(format!("array must have at least {} items", lower));
        }
      } else if let Some(upper) = upper {
        if values.len() > *upper {
          errors.push(format!("array must have not more than {} items", upper));
        }
      }

      iter_items = true;
    }
    Some(Occur::Optional(_)) => {
      if values.len() > 1 {
        errors.push("array must have 0 or 1 items".to_string());
      }

      iter_items = false;
    }
    None => {
      if values.is_empty() {
        errors.push("array must have exactly one item".to_string());
      } else {
        iter_items = false;
      }
    }
  }

  if !iter_items && !allow_empty_array {
    if let Some(entry_counts) = entry_counts {
      let len = values.len();
      if !validate_entry_count(entry_counts, len) {
        for ec in entry_counts.iter() {
          if let Some(occur) = &ec.entry_occurrence {
            errors.push(format!(
              "expecting array with length per occurrence {}",
              occur,
            ));
          } else {
            errors.push(format!(
              "expecting array with length {}, got {}",
              ec.count, len
            ));
          }
        }
      }
    }
  }

  if !errors.is_empty() {
    return Err(errors);
  }

  Ok((iter_items, allow_empty_array))
}

/// Retrieve number of group entries from a group choice. This is currently only
/// used for determining map equality/inequality and for validating the number
/// of entries in arrays, but may be useful in other contexts. The occurrence is
/// only captured for the second element of the CDDL array to avoid ambiguity in
/// non-homogenous array definitions
pub fn entry_counts_from_group_choice(cddl: &CDDL, group_choice: &GroupChoice) -> EntryCount {
  let mut count = 0;
  let mut entry_occurrence = None;

  for (idx, ge) in group_choice.group_entries.iter().enumerate() {
    match &ge.0 {
      GroupEntry::ValueMemberKey { ge, .. } => {
        if idx == 1 {
          if let Some(occur) = &ge.occur {
            entry_occurrence = Some(occur.occur.clone())
          }
        }

        count += 1;
      }
      GroupEntry::InlineGroup { group, occur, .. } => {
        if idx == 1 {
          if let Some(occur) = occur {
            entry_occurrence = Some(occur.occur.clone())
          }
        }
        for gc in group.group_choices.iter() {
          count += entry_counts_from_group_choice(cddl, gc).count;
        }
      }
      GroupEntry::TypeGroupname { ge, .. } => {
        if idx == 1 {
          if let Some(occur) = &ge.occur {
            entry_occurrence = Some(occur.occur.clone())
          }
        }
        if let Some(gr) = group_rule_from_ident(cddl, &ge.name) {
          count +=
            entry_counts_from_group_choice(cddl, &GroupChoice::new(vec![gr.entry.clone()])).count;
        } else {
          count += 1;
        }
      }
    }
  }

  EntryCount {
    count,
    entry_occurrence,
  }
}

/// Validate the number of entries given an array of possible valid entry counts
pub fn validate_entry_count(valid_entry_counts: &[EntryCount], num_entries: usize) -> bool {
  valid_entry_counts.iter().any(|ec| {
    num_entries == ec.count as usize
      || match ec.entry_occurrence {
        Some(Occur::ZeroOrMore(_)) | Some(Occur::Optional(_)) => true,
        Some(Occur::OneOrMore(_)) if num_entries > 0 => true,
        Some(Occur::Exact { lower, upper, .. }) => {
          if let Some(lower) = lower {
            if let Some(upper) = upper {
              num_entries >= lower && num_entries <= upper
            } else {
              num_entries >= lower
            }
          } else if let Some(upper) = upper {
            num_entries <= upper
          } else {
            false
          }
        }
        _ => false,
      }
  })
}

/// Entry count
#[derive(Clone, Debug)]
pub struct EntryCount {
  /// Count
  pub count: u64,
  /// Optional occurrence
  pub entry_occurrence: Option<Occur>,
}

/// Regex needs to be formatted in a certain way so it can be parsed. See
/// https://github.com/anweiss/cddl/issues/67
pub fn format_regex(input: &str) -> Option<String> {
  let mut formatted_regex = String::from(input);
  let mut unescape = Vec::new();
  for (idx, c) in formatted_regex.char_indices() {
    if c == '\\' {
      if let Some(c) = formatted_regex.chars().nth(idx + 1) {
        if !regex_syntax::is_meta_character(c) && c != 'd' {
          unescape.push(format!("\\{}", c));
        }
      }
    }
  }

  for replace in unescape.iter() {
    formatted_regex =
      formatted_regex.replace(replace, &replace.chars().nth(1).unwrap().to_string());
  }

  for find in ["?=", "?!", "?<=", "?<!"].iter() {
    if formatted_regex.contains(find) {
      return None;
    }
  }

  formatted_regex = formatted_regex.replace("?<", "?P<");

  Some(formatted_regex)
}

/// Concatenate target and controller
pub fn cat_operation<'a>(
  cddl: &CDDL,
  target: &Type2,
  controller: &Type2,
) -> Result<Vec<CATOperationResult<'a>>, String> {
  let mut literals = Vec::new();
  match target {
    Type2::TextValue { value, .. } => match controller {
      // "testing" .cat "123"
      Type2::TextValue {
        value: controller, ..
      } => literals.push(format!("{}{}", value, controller).into()),
      // "testing" .cat a
      Type2::Typename { ident, .. } => {
        for controller in string_literals_from_ident(cddl, ident).iter() {
          literals.append(&mut cat_operation(cddl, target, controller)?)
        }
      }
      // "testing" .cat '123'
      Type2::UTF8ByteString {
        value: controller, ..
      } => match std::str::from_utf8(controller) {
        Ok(controller) => literals.push(format!("{}{}", value, controller).into()),
        Err(e) => return Err(format!("error parsing byte string: {}", e)),
      },
      // "testing" .cat h'313233'
      Type2::B16ByteString {
        value: controller, ..
      } => match base16::decode(controller) {
        Ok(controller) => match String::from_utf8(controller) {
          Ok(controller) => literals.push(format!("{}{}", value, controller).into()),
          Err(e) => return Err(format!("error decoding utf-8: {}", e)),
        },
        Err(e) => return Err(format!("error decoding base16 byte string literal: {}", e)),
      },
      // "testing" .cat b64'MTIz'
      Type2::B64ByteString {
        value: controller, ..
      } => match base64::decode_config(controller, base64::URL_SAFE) {
        Ok(controller) => match String::from_utf8(controller) {
          Ok(controller) => literals.push(format!("{}{}", value, controller).into()),
          Err(e) => return Err(format!("error decoding utf-8: {}", e)),
        },
        Err(e) => {
          return Err(format!(
            "error decoding base64 encoded byte string literal: {}",
            e
          ))
        }
      },
      // "testing" .cat ( "123" / "1234" )
      Type2::ParenthesizedType { pt: controller, .. } => {
        for controller in controller.type_choices.iter() {
          if controller.type1.operator.is_none() {
            literals.append(&mut cat_operation(cddl, target, &controller.type1.type2)?);
          }
        }
      }
      _ => return Err("invalid controller used for .cat operation".to_string()),
    },
    // a .cat "123"
    Type2::Typename { ident, .. } => {
      // Only grab the first type choice literal from the target per
      // https://github.com/cbor-wg/cddl-control/issues/2#issuecomment-729253368
      if let Some(value) = string_literals_from_ident(cddl, ident).first() {
        literals.append(&mut cat_operation(cddl, value, controller)?);
      } else {
        return Err("invalid controller used for .cat operation".to_string());
      }
    }
    // ( "test" / "testing" ) .cat "123"
    Type2::ParenthesizedType { pt: target, .. } => {
      // Only grab the first type choice literal from the target per
      // https://github.com/cbor-wg/cddl-control/issues/2#issuecomment-729253368
      if let Some(tc) = target.type_choices.first() {
        // Ignore nested operator
        if tc.type1.operator.is_none() {
          literals.append(&mut cat_operation(cddl, &tc.type1.type2, controller)?);
        }
      }

      return Err("invalid target type in .cat control operator".to_string());
    }
    Type2::UTF8ByteString { value, .. } => match std::str::from_utf8(value) {
      Ok(value) => match controller {
        // 'testing' .cat "123"
        Type2::TextValue {
          value: controller, ..
        } => literals.push(format!("{}{}", value, controller).into()),
        Type2::Typename { ident, .. } => {
          for controller in string_literals_from_ident(cddl, ident).iter() {
            literals.append(&mut cat_operation(cddl, target, controller)?)
          }
        }
        // 'testing' .cat '123
        Type2::UTF8ByteString {
          value: controller, ..
        } => match std::str::from_utf8(controller) {
          Ok(controller) => literals.push(format!("{}{}", value, controller).into()),
          Err(e) => return Err(format!("error parsing byte string: {}", e)),
        },
        // 'testing' .cat h'313233'
        Type2::B16ByteString {
          value: controller, ..
        } => match base16::decode(controller) {
          Ok(controller) => match String::from_utf8(controller) {
            Ok(controller) => literals.push(format!("{}{}", value, controller).into()),
            Err(e) => return Err(format!("error decoding utf-8: {}", e)),
          },
          Err(e) => return Err(format!("error decoding base16 byte string literal: {}", e)),
        },
        // 'testing' .cat b64'MTIz'
        Type2::B64ByteString {
          value: controller, ..
        } => match base64::decode_config(controller, base64::URL_SAFE) {
          Ok(controller) => match String::from_utf8(controller) {
            Ok(controller) => literals.push(format!("{}{}", value, controller).into()),
            Err(e) => return Err(format!("error decoding utf-8: {}", e)),
          },
          Err(e) => {
            return Err(format!(
              "error decoding base64 encoded byte string literal: {}",
              e
            ))
          }
        },
        // 'testing' .cat ( "123" / "1234" )
        Type2::ParenthesizedType { pt: controller, .. } => {
          for controller in controller.type_choices.iter() {
            if controller.type1.operator.is_none() {
              literals.append(&mut cat_operation(cddl, target, &controller.type1.type2)?);
            }
          }
        }
        _ => return Err("invalid controller used for .cat operation".to_string()),
      },
      Err(e) => return Err(format!("error parsing byte string: {}", e)),
    },
    Type2::B16ByteString { value, .. } => match controller {
      // h'74657374696E67' .cat "123"
      Type2::TextValue {
        value: controller, ..
      } => {
        let controller = base16::encode_lower(&controller.as_bytes()[..]);
        let concat = [&value[..], &controller.as_bytes()[..]].concat();
        match base16::decode(&concat) {
          // Ignore the decoded value
          Ok(_) => literals.push(ByteValue::B16(concat.into()).into()),
          Err(e) => return Err(format!("concatenated value is invalid base16: {}", e)),
        }
      }
      // h'74657374696E67' .cat b
      Type2::Typename { ident, .. } => {
        for controller in string_literals_from_ident(cddl, ident).iter() {
          literals.append(&mut cat_operation(cddl, target, controller)?)
        }
      }
      // h'74657374696E67' .cat '123'
      Type2::UTF8ByteString {
        value: controller, ..
      } => {
        let controller = base16::encode_lower(&controller[..]);
        let concat = [&value[..], &controller.as_bytes()[..]].concat();
        match base16::decode(&concat) {
          // Ignore the decoded value
          Ok(_) => literals.push(ByteValue::B16(concat.into()).into()),
          Err(e) => return Err(format!("concatenated value is invalid base16: {}", e)),
        }
      }
      // h'74657374696E67' .cat h'313233'
      Type2::B16ByteString {
        value: controller, ..
      } => {
        let concat = [&value[..], &controller[..]].concat();
        match base16::decode(&concat) {
          // Ignore the decoded value
          Ok(_) => literals.push(ByteValue::B16(concat.into()).into()),
          Err(e) => return Err(format!("concatenated value is invalid base16: {}", e)),
        }
      }
      // h'74657374696E67' .cat b64'MTIz'
      Type2::B64ByteString {
        value: controller, ..
      } => match base64::decode_config(controller, base64::URL_SAFE) {
        Ok(controller) => {
          let controller = base16::encode_lower(&controller);
          let concat = [&value[..], &controller.as_bytes()[..]].concat();
          match base16::decode(&concat) {
            // Ignore the decoded value
            Ok(_) => literals.push(ByteValue::B16(concat.into()).into()),
            Err(e) => return Err(format!("concatenated value is invalid base16: {}", e)),
          }
        }
        Err(e) => return Err(format!("controller is invalid base64: {}", e)),
      },
      // h'74657374696E67' .cat ( "123" / "1234" )
      Type2::ParenthesizedType { pt: controller, .. } => {
        for controller in controller.type_choices.iter() {
          if controller.type1.operator.is_none() {
            literals.append(&mut cat_operation(cddl, target, &controller.type1.type2)?);
          }
        }
      }
      _ => return Err("invalid controller used for .cat operation".to_string()),
    },
    Type2::B64ByteString { value, .. } => match controller {
      // b64'dGVzdGluZw==' .cat "123"
      Type2::TextValue {
        value: controller, ..
      } => match base64::decode_config(value, base64::URL_SAFE) {
        Ok(value) => {
          let concat = [&value[..], &controller.as_bytes()[..]].concat();
          literals.push(
            ByteValue::B64(
              base64::encode_config(&concat, base64::URL_SAFE)
                .into_bytes()
                .into(),
            )
            .into(),
          )
        }
        Err(e) => return Err(format!("target is invalid base64: {}", e)),
      },
      // b64'dGVzdGluZw==' .cat b
      Type2::Typename { ident, .. } => {
        for controller in string_literals_from_ident(cddl, ident).iter() {
          literals.append(&mut cat_operation(cddl, target, controller)?)
        }
      }
      // b64'dGVzdGluZw==' .cat '123'
      Type2::UTF8ByteString {
        value: controller, ..
      } => match base64::decode_config(value, base64::URL_SAFE) {
        Ok(value) => {
          let concat = [&value[..], &controller[..]].concat();
          literals.push(
            ByteValue::B64(
              base64::encode_config(&concat, base64::URL_SAFE)
                .into_bytes()
                .into(),
            )
            .into(),
          )
        }
        Err(e) => return Err(format!("target is invalid base64: {}", e)),
      },
      // b64'dGVzdGluZw==' .cat h'313233'
      Type2::B16ByteString {
        value: controller, ..
      } => match base64::decode_config(value, base64::URL_SAFE) {
        Ok(value) => match base16::decode(controller) {
          Ok(controller) => {
            let concat = [&value[..], &controller[..]].concat();
            literals.push(
              ByteValue::B64(
                base64::encode_config(&concat, base64::URL_SAFE)
                  .into_bytes()
                  .into(),
              )
              .into(),
            )
          }
          Err(e) => return Err(format!("controller is invalid base16: {}", e)),
        },
        Err(e) => return Err(format!("target is invalid base64: {}", e)),
      },
      // b64'dGVzdGluZw==' .cat b64'MTIz'
      Type2::B64ByteString {
        value: controller, ..
      } => match base64::decode_config(value, base64::URL_SAFE) {
        Ok(value) => match base64::decode_config(controller, base64::URL_SAFE) {
          Ok(controller) => {
            let concat = [&value[..], &controller[..]].concat();
            literals.push(
              ByteValue::B64(
                base64::encode_config(&concat, base64::URL_SAFE)
                  .into_bytes()
                  .into(),
              )
              .into(),
            )
          }
          Err(e) => return Err(format!("controller is invalid base64: {}", e)),
        },
        Err(e) => return Err(format!("target is invalid base64: {}", e)),
      },
      // b64'dGVzdGluZw==' .cat ( "123" / "1234" )
      Type2::ParenthesizedType { pt: controller, .. } => {
        for controller in controller.type_choices.iter() {
          if controller.type1.operator.is_none() {
            literals.append(&mut cat_operation(cddl, target, &controller.type1.type2)?);
          }
        }
      }
      _ => return Err("invalid controller used for .cat operation".to_string()),
    },
    _ => return Err("invalid target used for .cat operation".to_string()),
  }

  Ok(literals)
}

/// Resulting data type from a cat operation
pub enum CATOperationResult<'a> {
  /// String result
  String(String),
  /// Bytes results
  Bytes(ByteValue<'a>),
}

impl<'a> From<String> for CATOperationResult<'a> {
  fn from(literal: String) -> Self {
    CATOperationResult::String(literal)
  }
}

impl<'a> From<ByteValue<'a>> for CATOperationResult<'a> {
  fn from(literal: ByteValue<'a>) -> Self {
    CATOperationResult::Bytes(literal)
  }
}
