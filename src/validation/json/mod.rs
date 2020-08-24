// Temporary
#![allow(missing_docs, unused_variables)]

mod controls;
mod visitor;

use super::{CompilationError, Error, Result, Validator};
use crate::{
  ast::*,
  lexer, parser,
  token::{self, Numeric, Token},
};
use controls::*;
use serde_json::{self, Value};
use std::{f64, fmt};

#[cfg(feature = "nightly")]
use std::convert::TryFrom;

#[cfg(feature = "nightly")]
use uriparse;

/// Error type when validating JSON
#[derive(Debug)]
pub struct JSONError {
  expected_memberkey: Option<String>,
  expected_value: String,
  actual_memberkey: Option<String>,
  actual_value: Value,
}

impl std::error::Error for JSONError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    None
  }
}

impl fmt::Display for JSONError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    let actual_value = serde_json::to_string_pretty(&self.actual_value).map_err(|_| fmt::Error)?;

    if let Some(emk) = &self.expected_memberkey {
      if let Some(amk) = &self.actual_memberkey {
        return write!(
          f,
          "expected: ( {} {} )\nactual: \"{}\": {}",
          emk, self.expected_value, amk, actual_value
        );
      }

      return write!(
        f,
        "expected: ( {} {} )\nactual: {}",
        emk, self.expected_value, actual_value
      );
    }

    if let Some(amk) = &self.actual_memberkey {
      return write!(
        f,
        "expected: ( {} )\nactual: {}: {}",
        self.expected_value, amk, actual_value
      );
    }

    write!(
      f,
      "expected: ( {} )\nactual: {}\n",
      self.expected_value, actual_value,
    )
  }
}

impl Into<Error> for JSONError {
  fn into(self) -> Error {
    Error::Target(Box::from(self))
  }
}

impl<'a> Validator<Value> for CDDL<'a> {
  fn validate(&self, value: &Value) -> Result {
    for r in self.rules.iter() {
      // First type rule is root
      if let Rule::Type { rule, .. } = r {
        return self.validate_type_rule(rule, None, None, None, value);
      }
    }

    Ok(())
  }

  fn validate_rule_for_ident(
    &self,
    ident: &Identifier,
    is_enumeration: bool,
    expected_memberkey: Option<String>,
    actual_memberkey: Option<String>,
    occur: Option<&Occurrence>,
    value: &Value,
  ) -> Result {
    for rule in self.rules.iter() {
      match rule {
        Rule::Type { rule, .. } if rule.name.ident == ident.ident => {
          return self.validate_type_rule(&rule, expected_memberkey, actual_memberkey, occur, value)
        }
        Rule::Group { rule, .. } if rule.name.ident == ident.ident => {
          return self.validate_group_rule(&rule, is_enumeration, occur, value)
        }
        _ => continue,
      }
    }

    Err(Error::Syntax(format!(
      "No rule with name \"{}\" defined",
      ident.ident
    )))
  }

  fn validate_type_rule(
    &self,
    tr: &TypeRule,
    expected_memberkey: Option<String>,
    actual_memberkey: Option<String>,
    occur: Option<&Occurrence>,
    value: &Value,
  ) -> Result {
    self.validate_type(
      &tr.value,
      expected_memberkey,
      actual_memberkey,
      occur,
      value,
    )
  }

  fn validate_group_rule(
    &self,
    gr: &GroupRule,
    is_enumeration: bool,
    occur: Option<&Occurrence>,
    value: &Value,
  ) -> Result {
    self.validate_group_entry(&gr.entry, is_enumeration, None, occur, value)
  }

  fn validate_type(
    &self,
    t: &Type,
    expected_memberkey: Option<String>,
    actual_memberkey: Option<String>,
    occur: Option<&Occurrence>,
    value: &Value,
  ) -> Result {
    let mut validation_errors: Vec<Error> = Vec::new();

    // Find the first type choice that validates to true
    let find_type_choice = |tc: &TypeChoice| match self.validate_type1(
      &tc.type1,
      expected_memberkey.clone(),
      actual_memberkey.clone(),
      occur,
      value,
    ) {
      Ok(()) => true,
      Err(e) => {
        validation_errors.push(e);
        false
      }
    };

    if t.type_choices.iter().any(find_type_choice) {
      return Ok(());
    }

    Err(Error::MultiError(validation_errors))
  }

  fn validate_type1(
    &self,
    t1: &Type1,
    expected_memberkey: Option<String>,
    actual_memberkey: Option<String>,
    occur: Option<&Occurrence>,
    value: &Value,
  ) -> Result {
    if let Some(Operator {
      operator: rco,
      type2: t2,
      ..
    }) = &t1.operator
    {
      match rco {
        RangeCtlOp::RangeOp { is_inclusive, .. } => {
          return self.validate_range(&t1.type2, t2, *is_inclusive, value)
        }
        RangeCtlOp::CtlOp { ctrl, .. } => {
          return self.validate_control_operator(&t1.type2, ctrl, t2, value)
        }
      }
    }

    self.validate_type2(
      &t1.type2,
      expected_memberkey,
      actual_memberkey,
      occur,
      value,
    )
  }

  // TODO: Reduce cognitive complexity of this function
  #[allow(clippy::cognitive_complexity)]
  fn validate_range(
    &self,
    lower: &Type2,
    upper: &Type2,
    is_inclusive: bool,
    value: &Value,
  ) -> Result {
    if let Value::Number(n) = value {
      // TODO: Per spec, if lower bound exceeds upper bound, resulting type is
      // empty set. Not sure how this translates to numerical JSON validation.
      match lower {
        Type2::IntValue { value: li, .. } => match upper {
          Type2::IntValue { value: ui, .. } => match n.as_i64() {
            Some(ni) if is_inclusive => {
              if ni >= *li as i64 && ni <= *ui as i64 {
                Ok(())
              } else {
                Err(
                  JSONError {
                    expected_memberkey: None,
                    expected_value: format!("Range: {} <= value <= {}", li, ui),
                    actual_memberkey: None,
                    actual_value: value.clone(),
                  }
                  .into(),
                )
              }
            }
            Some(ni) => {
              if ni >= *li as i64 && ni < *ui as i64 {
                Ok(())
              } else {
                Err(
                  JSONError {
                    expected_memberkey: None,
                    expected_value: format!("Range: {} <= value < {}", li, ui),
                    actual_memberkey: None,
                    actual_value: value.clone(),
                  }
                  .into(),
                )
              }
            }
            None => Err(
              JSONError {
                expected_memberkey: None,
                expected_value: format!("Range: {} <= value <= {}", li, ui),
                actual_memberkey: None,
                actual_value: value.clone(),
              }
              .into(),
            ),
          },
          Type2::UintValue { value: ui, .. } => match n.as_i64() {
            Some(ni) if is_inclusive => {
              if ni >= *li as i64 && ni <= *ui as i64 {
                Ok(())
              } else {
                Err(
                  JSONError {
                    expected_memberkey: None,
                    expected_value: format!("Range: {} <= value <= {}", li, ui),
                    actual_memberkey: None,
                    actual_value: value.clone(),
                  }
                  .into(),
                )
              }
            }
            Some(ni) => {
              if ni >= *li as i64 && ni < *ui as i64 {
                Ok(())
              } else {
                Err(
                  JSONError {
                    expected_memberkey: None,
                    expected_value: format!("Range: {} <= value < {}", li, ui),
                    actual_memberkey: None,
                    actual_value: value.clone(),
                  }
                  .into(),
                )
              }
            }
            None => Err(
              JSONError {
                expected_memberkey: None,
                expected_value: format!("Range between {} and {}", li, ui),
                actual_memberkey: None,
                actual_value: value.clone(),
              }
              .into(),
            ),
          },
          Type2::Typename { ident, .. } => match self.numerical_value_type_from_ident(ident) {
            Some(t2) => {
              let mut validation_errors: Vec<Error> = Vec::new();

              if t2.iter().any(
                |tc| match self.validate_range(lower, tc, is_inclusive, value) {
                  Ok(()) => true,
                  Err(e) => {
                    validation_errors.push(e);
                    false
                  }
                },
              ) {
                Ok(())
              } else {
                Err(Error::MultiError(validation_errors))
              }
            }
            None => Err(Error::Syntax(format!(
              "Invalid upper range value. Type {} not defined",
              ident
            ))),
          },
          _ => Err(Error::Syntax(format!(
            "Invalid upper range value: Got {}",
            upper
          ))),
        },
        Type2::UintValue { value: li, .. } => match upper {
          Type2::UintValue { value: ui, .. } => match n.as_u64() {
            Some(ni) if is_inclusive => {
              if ni >= *li as u64 && ni <= *ui as u64 {
                Ok(())
              } else {
                Err(
                  JSONError {
                    expected_memberkey: None,
                    expected_value: format!("Range: {} <= value <= {}", li, ui),
                    actual_memberkey: None,
                    actual_value: value.clone(),
                  }
                  .into(),
                )
              }
            }
            Some(ni) => {
              if ni >= *li as u64 && ni < *ui as u64 {
                Ok(())
              } else {
                Err(
                  JSONError {
                    expected_memberkey: None,
                    expected_value: format!("Range: {} <= value < {}", li, ui),
                    actual_memberkey: None,
                    actual_value: value.clone(),
                  }
                  .into(),
                )
              }
            }
            None => Err(
              JSONError {
                expected_memberkey: None,
                expected_value: format!("Range between {} and {}", li, ui),
                actual_memberkey: None,
                actual_value: value.clone(),
              }
              .into(),
            ),
          },
          Type2::Typename { ident, .. } => match self.numerical_value_type_from_ident(ident) {
            Some(t2) => {
              let mut validation_errors: Vec<Error> = Vec::new();

              if t2.iter().any(
                |tc| match self.validate_range(lower, tc, is_inclusive, value) {
                  Ok(()) => true,
                  Err(e) => {
                    validation_errors.push(e);
                    false
                  }
                },
              ) {
                Ok(())
              } else {
                Err(Error::MultiError(validation_errors))
              }
            }
            None => Err(Error::Syntax(format!(
              "Invalid upper range value. Type {} not defined",
              ident
            ))),
          },
          _ => Err(Error::Syntax(format!(
            "Invalid upper range value: Got {}",
            upper
          ))),
        },
        Type2::FloatValue { value: lf, .. } => match upper {
          Type2::FloatValue { value: uf, .. } => match n.as_f64() {
            Some(nf) if is_inclusive => {
              if nf >= *lf && nf <= *uf {
                Ok(())
              } else {
                Err(
                  JSONError {
                    expected_memberkey: None,
                    expected_value: format!("Range: {} <= value <= {}", lf, uf),
                    actual_memberkey: None,
                    actual_value: value.clone(),
                  }
                  .into(),
                )
              }
            }
            Some(nf) => {
              if nf >= *lf && nf < *uf {
                Ok(())
              } else {
                Err(
                  JSONError {
                    expected_memberkey: None,
                    expected_value: format!("Range: {} <= value < {}", lf, uf),
                    actual_memberkey: None,
                    actual_value: value.clone(),
                  }
                  .into(),
                )
              }
            }
            None => Err(
              JSONError {
                expected_memberkey: None,
                expected_value: format!("Range between {} and {}", lf, uf),
                actual_memberkey: None,
                actual_value: value.clone(),
              }
              .into(),
            ),
          },
          Type2::Typename { ident, .. } => match self.numerical_value_type_from_ident(ident) {
            Some(t2) => {
              let mut validation_errors: Vec<Error> = Vec::new();

              if t2.iter().any(
                |tc| match self.validate_range(lower, tc, is_inclusive, value) {
                  Ok(()) => true,
                  Err(e) => {
                    validation_errors.push(e);
                    false
                  }
                },
              ) {
                Ok(())
              } else {
                Err(Error::MultiError(validation_errors))
              }
            }
            None => Err(Error::Syntax(format!(
              "Invalid upper range value. Type {} not defined",
              ident
            ))),
          },
          _ => Err(Error::Syntax(format!(
            "Invalid upper range value: Got {}",
            upper
          ))),
        },
        Type2::Typename { ident, .. } => match self.numerical_value_type_from_ident(ident) {
          Some(t2) => {
            let mut validation_errors: Vec<Error> = Vec::new();

            if t2.iter().any(
              |tc| match self.validate_range(tc, upper, is_inclusive, value) {
                Ok(()) => true,
                Err(e) => {
                  validation_errors.push(e);
                  false
                }
              },
            ) {
              Ok(())
            } else {
              Err(Error::MultiError(validation_errors))
            }
          }
          None => Err(Error::Syntax(format!(
            "Invalid lower range value. Type with name {} not defined",
            lower
          ))),
        },
        _ => Err(Error::Syntax(format!(
          "Invalid lower range value: Got {}",
          lower
        ))),
      }
    } else {
      Err(
        JSONError {
          expected_memberkey: None,
          expected_value: format!("Expected numerical value between {} and {}", lower, upper),
          actual_memberkey: None,
          actual_value: value.clone(),
        }
        .into(),
      )
    }
  }

  fn validate_control_operator(
    &self,
    target: &Type2,
    operator: &str,
    controller: &Type2,
    value: &Value,
  ) -> Result {
    let mut errors: Vec<Error> = Vec::new();

    match token::lookup_control_from_str(operator) {
      t @ Some(Token::PCRE) | t @ Some(Token::CREGEXP) => {
        if t == Some(Token::CREGEXP) {
          println!("NOTE: Only Perl-compatible regex is supported.\nThis crate evaluates the .regexp operator as an alias for the .pcre extension operator\n");
        }

        if !self.is_type_string_data_type(target) {
          return Err(Error::Syntax(format!(
            "the {} control operator is only defined for the text type. Got {}",
            Token::PCRE,
            target
          )));
        }

        let find_valid_value = |c: &str| -> bool {
          match validate_pcre_control(&c, value) {
            Ok(()) => true,
            Err(e) => {
              errors.push(e);

              false
            }
          }
        };

        if self
          .text_values_from_type(controller)?
          .into_iter()
          .any(find_valid_value)
        {
          Ok(())
        } else {
          Err(Error::MultiError(errors))
        }
      }
      Some(Token::LT) => {
        if !self.is_type_numeric_data_type(target) {
          return Err(Error::Syntax(format!(
            "the {} control operator is only defined for the numeric type. Got {}",
            Token::LT,
            target
          )));
        }

        let find_valid_value = |n: Numeric| -> bool {
          match validate_lt_control(n, value) {
            Ok(()) => true,
            Err(e) => {
              errors.push(e);

              false
            }
          }
        };

        if self
          .numeric_values_from_type(target, controller)?
          .into_iter()
          .any(find_valid_value)
        {
          Ok(())
        } else {
          Err(Error::MultiError(errors))
        }
      }
      Some(Token::LE) => {
        if !self.is_type_numeric_data_type(target) {
          return Err(Error::Syntax(format!(
            "the {} control operator is only defined for the numeric type. Got {}",
            Token::LT,
            target
          )));
        }

        let find_valid_value = |n: Numeric| -> bool {
          match validate_le_control(n, value) {
            Ok(()) => true,
            Err(e) => {
              errors.push(e);

              false
            }
          }
        };

        if self
          .numeric_values_from_type(target, controller)?
          .into_iter()
          .any(find_valid_value)
        {
          Ok(())
        } else {
          Err(Error::MultiError(errors))
        }
      }
      Some(Token::GT) => {
        if !self.is_type_numeric_data_type(target) {
          return Err(Error::Syntax(format!(
            "the {} control operator is only defined for the numeric type. Got {}",
            Token::LT,
            target
          )));
        }

        let find_valid_value = |n: Numeric| -> bool {
          match validate_gt_control(n, value) {
            Ok(()) => true,
            Err(e) => {
              errors.push(e);

              false
            }
          }
        };

        if self
          .numeric_values_from_type(target, controller)?
          .into_iter()
          .any(find_valid_value)
        {
          Ok(())
        } else {
          Err(Error::MultiError(errors))
        }
      }
      Some(Token::GE) => {
        if !self.is_type_numeric_data_type(target) {
          return Err(Error::Syntax(format!(
            "the {} control operator is only defined for the numeric type. Got {}",
            Token::LT,
            target
          )));
        }

        let find_valid_value = |n: Numeric| -> bool {
          match validate_ge_control(n, value) {
            Ok(()) => true,
            Err(e) => {
              errors.push(e);

              false
            }
          }
        };

        if self
          .numeric_values_from_type(target, controller)?
          .into_iter()
          .any(find_valid_value)
        {
          Ok(())
        } else {
          Err(Error::MultiError(errors))
        }
      }
      Some(Token::EQ) => {
        if self.is_type_numeric_data_type(target) {
          let find_valid_value = |n: Numeric| -> bool {
            match validate_eq_numeric_control(n, value) {
              Ok(()) => true,
              Err(e) => {
                errors.push(e);

                false
              }
            }
          };

          if self
            .numeric_values_from_type(target, controller)?
            .into_iter()
            .any(find_valid_value)
          {
            Ok(())
          } else {
            Err(Error::MultiError(errors))
          }
        } else if self.is_type_string_data_type(target) {
          let find_valid_value = |c: &str| -> bool {
            match validate_eq_text_control(&c, value) {
              Ok(()) => true,
              Err(e) => {
                errors.push(e);

                false
              }
            }
          };

          if self
            .text_values_from_type(controller)?
            .into_iter()
            .any(find_valid_value)
          {
            Ok(())
          } else {
            Err(Error::MultiError(errors))
          }
        } else {
          unimplemented!()
        }
      }
      _ => unimplemented!(),
    }
  }

  fn validate_type2(
    &self,
    t2: &Type2,
    expected_memberkey: Option<String>,
    actual_memberkey: Option<String>,
    occur: Option<&Occurrence>,
    value: &Value,
  ) -> Result {
    match t2 {
      Type2::TextValue { value: t, .. } => match value {
        Value::String(s) if t == s => Ok(()),
        _ => Err(
          JSONError {
            expected_memberkey,
            expected_value: t2.to_string(),
            actual_memberkey,
            actual_value: value.clone(),
          }
          .into(),
        ),
      },
      Type2::IntValue { .. } | Type2::UintValue { .. } | Type2::FloatValue { .. } => match value {
        Value::Number(_) => validate_numeric_value(t2, value),
        _ => Err(
          JSONError {
            expected_memberkey,
            expected_value: t2.to_string(),
            actual_memberkey,
            actual_value: value.clone(),
          }
          .into(),
        ),
      },
      // If type name identifier is 'any'
      Type2::Typename { ident, .. } if ident.ident == "any" => Ok(()),
      // TODO: evaluate genericarg
      Type2::Typename { ident, .. } => match value {
        Value::Null => expect_null(&ident.ident),
        Value::Bool(_) => self.expect_bool(&ident.ident, value),
        Value::String(s) => match ident.ident {
          "tstr" | "text" => Ok(()),
          "tdate" => validate_tdate(s),
          #[cfg(feature = "nightly")]
          "uri" => validate_uri(s),
          #[cfg(not(feature = "nightly"))]
          "uri" => {
            println!("NOTE: Validation against the \"uri\" data type is only supported by versions of this crate built with unstable Rust\n");

            Ok(())
          }
          _ => {
            if is_type_json_prelude(&ident.ident) {
              return Err(
                JSONError {
                  expected_memberkey,
                  expected_value: ident.ident.to_string(),
                  actual_memberkey,
                  actual_value: value.clone(),
                }
                .into(),
              );
            }

            self.validate_rule_for_ident(
              ident,
              false,
              expected_memberkey,
              actual_memberkey,
              occur,
              value,
            )
          }
        },
        Value::Number(_) => {
          self.validate_numeric_data_type(expected_memberkey, actual_memberkey, &ident.ident, value)
        }
        Value::Object(_) => self.validate_rule_for_ident(
          ident,
          false,
          expected_memberkey,
          actual_memberkey,
          occur,
          value,
        ),
        Value::Array(_) => self.validate_rule_for_ident(
          ident,
          false,
          expected_memberkey,
          actual_memberkey,
          occur,
          value,
        ),
      },
      Type2::Array { group, .. } => match value {
        Value::Array(_) => self.validate_group(group, occur, value),
        _ => Err(
          JSONError {
            expected_memberkey,
            expected_value: t2.to_string(),
            actual_memberkey,
            actual_value: value.clone(),
          }
          .into(),
        ),
      },
      Type2::Map { group, .. } => match value {
        Value::Object(_) => self.validate_group(group, occur, value),
        _ => Err(
          JSONError {
            expected_memberkey,
            expected_value: t2.to_string(),
            actual_memberkey,
            actual_value: value.clone(),
          }
          .into(),
        ),
      },
      Type2::ChoiceFromInlineGroup { group, .. } => {
        self.validate_group_to_choice_enum(group, occur, value)
      }
      Type2::ChoiceFromGroup { ident, .. } => self.validate_rule_for_ident(
        ident,
        true,
        expected_memberkey,
        actual_memberkey,
        occur,
        value,
      ),
      _ => Err(Error::Syntax(format!(
        "CDDL type {} can't be used to validate JSON {}",
        t2, value
      ))),
    }
  }

  fn validate_group_to_choice_enum(
    &self,
    g: &Group,
    occur: Option<&Occurrence>,
    value: &Value,
  ) -> Result {
    let mut validation_errors: Vec<Error> = Vec::new();

    let validate_type_from_group_entry = |gc: &GroupChoice| {
      gc.group_entries.iter().any(|ge| match &ge.0 {
        // Member names have only "documentary value" when evaluating an
        // enumeration expression
        GroupEntry::ValueMemberKey { ge, .. } => {
          match self.validate_type(&ge.entry_type, None, None, occur, value) {
            Ok(()) => true,
            Err(e) => {
              validation_errors.push(e);
              false
            }
          }
        }
        _ => false,
      })
    };

    if g.group_choices.iter().any(validate_type_from_group_entry) {
      return Ok(());
    };

    Err(Error::MultiError(validation_errors))
  }

  fn validate_group(&self, g: &Group, occur: Option<&Occurrence>, value: &Value) -> Result {
    let mut validation_errors: Vec<Error> = Vec::new();

    // Find the first group choice that validates to true
    if g
      .group_choices
      .iter()
      .any(|gc| match self.validate_group_choice(gc, occur, value) {
        Ok(()) => true,
        Err(e) => {
          validation_errors.push(e);
          false
        }
      })
    {
      return Ok(());
    }

    Err(Error::MultiError(validation_errors))
  }

  fn validate_group_choice(
    &self,
    gc: &GroupChoice,
    occur: Option<&Occurrence>,
    value: &Value,
  ) -> Result {
    let mut errors: Vec<Error> = Vec::new();

    // Check for a wildcard entry
    // * tstr => any
    let wildcard_entry = gc.group_entries.iter().find_map(|ge| match &ge.0 {
      GroupEntry::ValueMemberKey { ge, .. } => {
        if let Some(Occurrence {
          occur: Occur::ZeroOrMore(_),
          ..
        }) = ge.occur
        {
          match &ge.member_key {
            Some(MemberKey::Type1 { t1, is_cut, .. }) if !is_cut => match &t1.type2 {
              Type2::Typename {
                ident,
                generic_args: None,
                ..
              } if ident.ident == "tstr" => Some(&ge.entry_type),
              _ => None,
            },
            _ => None,
          }
        } else {
          None
        }
      }
      _ => None,
    });

    for (idx, ge) in gc.group_entries.iter().enumerate() {
      match value {
        Value::Array(values) => {
          // [ ( a: int, b: tstr ) ]
          if let GroupEntry::InlineGroup { group, .. } = &ge.0 {
            self.validate_group(group, None, value)?;
            continue;
          }

          if let GroupEntry::TypeGroupname { ge: tge, .. } = &ge.0 {
            // [ * reputon ]
            if gc.group_entries.len() == 1 {
              if let Some(Occurrence { occur: o, .. }) = &tge.occur {
                self.validate_array_occurrence(o, &tge.name.to_string(), values)?;

                if let Occur::ZeroOrMore(_) = o {
                  if values.is_empty() {
                    return Ok(());
                  }
                }

                if is_type_json_prelude(&tge.name.ident) {
                  let validate_all_values = |v| {
                    self
                      .validate_type2(
                        &Type2::Typename {
                          ident: tge.name.clone(),
                          generic_args: tge.generic_args.clone(),
                          span: (0, 0, 0),
                        },
                        None,
                        None,
                        None,
                        v,
                      )
                      .is_ok()
                  };

                  if values.iter().all(validate_all_values) {
                    return Ok(());
                  } else {
                    return Err(
                      JSONError {
                        expected_memberkey: None,
                        expected_value: gc.to_string(),
                        actual_memberkey: None,
                        actual_value: value.clone(),
                      }
                      .into(),
                    );
                  }
                }

                let validate_all_values = |v| {
                  self
                    .validate_rule_for_ident(&tge.name, false, None, None, None, v)
                    .is_ok()
                };

                if values.iter().all(validate_all_values) {
                  return Ok(());
                } else {
                  return Err(
                    JSONError {
                      expected_memberkey: None,
                      expected_value: gc.to_string(),
                      actual_memberkey: None,
                      actual_value: value.clone(),
                    }
                    .into(),
                  );
                }
              }
            }
          }

          // [ a: int, b: tstr ]
          if let GroupEntry::ValueMemberKey { ge: vmke, .. } = &ge.0 {
            // Ignore name/value entries with an occurrence indicator to avoid ambiguity
            if vmke.occur.is_none() {
              if let Some(v) = values.get(idx) {
                self.validate_group_entry(&ge.0, false, None, occur, v)?;
              }
            }
          }
        }
        // Validate the object key/value pairs against each group entry,
        // collecting errors along the way
        Value::Object(_) => {
          match self.validate_group_entry(&ge.0, false, wildcard_entry, occur, value) {
            Ok(()) => continue,
            Err(e) => errors.push(e),
          }
        }
        _ => {
          return Err(
            JSONError {
              expected_memberkey: None,
              expected_value: gc.to_string(),
              actual_memberkey: None,
              actual_value: value.clone(),
            }
            .into(),
          );
        }
      }
    }

    if !errors.is_empty() {
      return Err(Error::MultiError(errors));
    }

    Ok(())
  }

  fn validate_group_entry(
    &self,
    ge: &GroupEntry,
    is_enumeration: bool,
    wildcard_entry: Option<&Type>,
    occur: Option<&Occurrence>,
    value: &Value,
  ) -> Result {
    match ge {
      GroupEntry::ValueMemberKey { ge: vmke, .. } => {
        if let Some(mk) = &vmke.member_key {
          match mk {
            MemberKey::Type1 { t1, is_cut, .. } => match &t1.type2 {
              // CDDL { "my-key" => tstr, } validates JSON { "my-key": "myvalue" }
              Type2::TextValue { value: t, .. } => match value {
                Value::Object(om) => {
                  if !is_type_json_prelude(&vmke.entry_type.to_string()) {
                    if let Some(v) = om.get(*t) {
                      return self.validate_type(
                        &vmke.entry_type,
                        Some(mk.to_string()),
                        Some((*t).to_string()),
                        occur,
                        v,
                      );
                    }

                    return self.validate_type(
                      &vmke.entry_type,
                      Some(mk.to_string()),
                      None,
                      occur,
                      value,
                    );
                  }

                  if let Some(v) = om.get(*t) {
                    let r = self.validate_type(
                      &vmke.entry_type,
                      Some(mk.to_string()),
                      Some((*t).to_string()),
                      occur,
                      v,
                    );

                    if r.is_err() && !is_cut {
                      if let Some(entry_type) = wildcard_entry {
                        return self.validate_type(
                          entry_type,
                          Some(mk.to_string()),
                          Some((*t).to_string()),
                          occur,
                          v,
                        );
                      }
                    }

                    r
                  } else {
                    Err(
                      JSONError {
                        expected_memberkey: Some(mk.to_string()),
                        expected_value: ge.to_string(),
                        actual_memberkey: None,
                        actual_value: value.clone(),
                      }
                      .into(),
                    )
                  }
                }
                // Otherwise, validate JSON against the type of the entry.
                // Matched when in an array and the key for the group entry is
                // ignored.
                // CDDL [ city: tstr, ] validates JSON [ "city" ]
                _ => self.validate_type(&vmke.entry_type, Some(mk.to_string()), None, occur, value),
              },

              // CDDL { * tstr => any } validates { "otherkey1": "anyvalue", "otherkey2": true }
              Type2::Typename { ident, .. } if ident.ident == "tstr" || ident.ident == "text" => {
                Ok(())
              }
              _ => Err(Error::Syntax(
                "CDDL member key must be quoted string or bareword for validating JSON objects"
                  .to_string(),
              )),
            },
            MemberKey::Bareword { ident, .. } => match value {
              Value::Object(om) => {
                if !is_type_json_prelude(&vmke.entry_type.to_string()) {
                  if let Some(v) = om.get(ident.ident) {
                    return self.validate_type(
                      &vmke.entry_type,
                      Some(mk.to_string()),
                      Some((ident.ident).to_string()),
                      vmke.occur.as_ref(),
                      v,
                    );
                  }

                  return self.validate_type(
                    &vmke.entry_type,
                    Some(mk.to_string()),
                    None,
                    vmke.occur.as_ref(),
                    value,
                  );
                }

                match om.get(ident.ident) {
                  Some(v) => self.validate_type(
                    &vmke.entry_type,
                    Some(mk.to_string()),
                    Some(ident.ident.to_string()),
                    vmke.occur.as_ref(),
                    v,
                  ),
                  None => match occur {
                    Some(Occurrence { occur: o, .. }) => match o {
                      Occur::Optional(_) | Occur::OneOrMore(_) => Ok(()),
                      _ => Err(
                        JSONError {
                          expected_memberkey: Some(mk.to_string()),
                          expected_value: format!("{} {}", mk, vmke.entry_type),
                          actual_memberkey: None,
                          actual_value: value.clone(),
                        }
                        .into(),
                      ),
                    },
                    None => Err(
                      JSONError {
                        expected_memberkey: Some(mk.to_string()),
                        expected_value: format!("{} {}", mk, vmke.entry_type),
                        actual_memberkey: None,
                        actual_value: value.clone(),
                      }
                      .into(),
                    ),
                  },
                }
              }
              _ => self.validate_type(
                &vmke.entry_type,
                Some(mk.to_string()),
                None,
                vmke.occur.as_ref(),
                value,
              ),
            },
            _ => Err(Error::Syntax(
              "CDDL member key must be quoted string or bareword for validating JSON objects"
                .to_string(),
            )),
          }
        } else {
          self.validate_type(&vmke.entry_type, None, None, occur, value)
        }
      }
      GroupEntry::TypeGroupname { ge: tge, .. } => self.validate_rule_for_ident(
        &tge.name,
        is_enumeration,
        None,
        None,
        tge.occur.as_ref(),
        value,
      ),
      GroupEntry::InlineGroup {
        occur: igo,
        group: g,
        ..
      } => {
        if igo.is_some() {
          if is_enumeration {
            return self.validate_group_to_choice_enum(g, igo.as_ref(), value);
          }

          self.validate_group(g, igo.as_ref(), value)
        } else {
          if is_enumeration {
            return self.validate_group_to_choice_enum(g, occur, value);
          }
          self.validate_group(g, occur, value)
        }
      }
    }
  }

  fn validate_array_occurrence(&self, occur: &Occur, group: &str, values: &[Value]) -> Result {
    match occur {
      Occur::ZeroOrMore(_) | Occur::Optional(_) => Ok(()),
      Occur::OneOrMore(_) => {
        if values.is_empty() {
          Err(Error::Occurrence(format!(
            "Expecting one or more values of group {}",
            group
          )))
        } else {
          Ok(())
        }
      }
      Occur::Exact { lower, upper, .. } => {
        if let Some(li) = lower {
          if let Some(ui) = upper {
            if values.len() < *li || values.len() > *ui {
              if li == ui {
                return Err(Error::Occurrence(format!(
                  "Expecting exactly {} values of group {}. Got {} values",
                  li,
                  group,
                  values.len()
                )));
              }

              return Err(Error::Occurrence(format!(
                "Expecting between {} and {} values of group {}. Got {} values",
                li,
                ui,
                group,
                values.len()
              )));
            }
          }

          if values.len() < *li {
            return Err(Error::Occurrence(format!(
              "Expecting at least {} values of group {}. Got {} values",
              li,
              group,
              values.len()
            )));
          }
        }

        if let Some(ui) = upper {
          if values.len() > *ui {
            return Err(Error::Occurrence(format!(
              "Expecting no more than {} values of group {}. Got {} values",
              ui,
              group,
              values.len()
            )));
          }
        }

        Ok(())
      }
    }
  }

  fn expect_bool(&self, ident: &str, value: &Value) -> Result {
    match value {
      Value::Bool(b) => {
        if ident == "bool" {
          return Ok(());
        }

        if let Ok(bfs) = ident.parse::<bool>() {
          if bfs == *b {
            return Ok(());
          }

          return Err(
            JSONError {
              expected_memberkey: None,
              expected_value: ident.to_string(),
              actual_memberkey: None,
              actual_value: value.clone(),
            }
            .into(),
          );
        }

        Err(
          JSONError {
            expected_memberkey: None,
            expected_value: ident.to_string(),
            actual_memberkey: None,
            actual_value: value.clone(),
          }
          .into(),
        )
      }
      _ => Err(
        JSONError {
          expected_memberkey: None,
          expected_value: ident.to_string(),
          actual_memberkey: None,
          actual_value: value.clone(),
        }
        .into(),
      ),
    }
  }

  fn validate_numeric_data_type(
    &self,
    expected_memberkey: Option<String>,
    actual_memberkey: Option<String>,
    ident: &str,
    value: &Value,
  ) -> Result {
    match value {
      Value::Number(n) => match ident {
        "uint" => n
          .as_u64()
          .ok_or_else(|| {
            JSONError {
              expected_memberkey,
              expected_value: ident.to_string(),
              actual_memberkey,
              actual_value: value.clone(),
            }
            .into()
          })
          .map(|_| ()),
        "nint" => match n.as_i64() {
          Some(n64) if n64 < 0 => Ok(()),
          _ => Err(
            JSONError {
              expected_memberkey,
              expected_value: ident.to_string(),
              actual_memberkey,
              actual_value: value.clone(),
            }
            .into(),
          ),
        },
        "int" => n
          .as_i64()
          .ok_or_else(|| {
            JSONError {
              expected_memberkey,
              expected_value: ident.to_string(),
              actual_memberkey,
              actual_value: value.clone(),
            }
            .into()
          })
          .map(|_| ()),
        "number" => Ok(()),
        "float16" => match n.as_f64() {
          Some(_) => Ok(()),
          _ => Err(
            JSONError {
              expected_memberkey,
              expected_value: ident.to_string(),
              actual_memberkey,
              actual_value: value.clone(),
            }
            .into(),
          ),
        },
        // TODO: Finish rest of numerical data types
        "float32" => match n.as_f64() {
          Some(_) => Ok(()),
          _ => Err(
            JSONError {
              expected_memberkey,
              expected_value: ident.to_string(),
              actual_memberkey,
              actual_value: value.clone(),
            }
            .into(),
          ),
        },
        // TODO: Finish rest of numerical data types
        _ => Err(
          JSONError {
            expected_memberkey,
            expected_value: ident.to_string(),
            actual_memberkey,
            actual_value: value.clone(),
          }
          .into(),
        ),
      },
      _ => Err(
        JSONError {
          expected_memberkey,
          expected_value: ident.to_string(),
          actual_memberkey,
          actual_value: value.clone(),
        }
        .into(),
      ),
    }
  }
}

fn validate_numeric_value(t2: &Type2, value: &Value) -> Result {
  match value {
    Value::Number(n) => match *t2 {
      Type2::IntValue { value: i, .. } => match n.as_i64() {
        Some(n64) if n64 == i as i64 => Ok(()),
        _ => Err(
          JSONError {
            expected_memberkey: None,
            expected_value: t2.to_string(),
            actual_memberkey: None,
            actual_value: value.clone(),
          }
          .into(),
        ),
      },
      Type2::FloatValue { value: f, .. } => match n.as_f64() {
        Some(n64) if (n64 - f as f64).abs() < f64::EPSILON => Ok(()),
        _ => Err(
          JSONError {
            expected_memberkey: None,
            expected_value: t2.to_string(),
            actual_memberkey: None,
            actual_value: value.clone(),
          }
          .into(),
        ),
      },
      _ => Ok(()),
    },
    // Expecting a numerical value but got different type
    _ => Err(
      JSONError {
        expected_memberkey: None,
        expected_value: t2.to_string(),
        actual_memberkey: None,
        actual_value: value.clone(),
      }
      .into(),
    ),
  }
}

fn expect_null(ident: &str) -> Result {
  match ident {
    "null" | "nil" => Ok(()),
    _ => Err(
      JSONError {
        expected_memberkey: None,
        expected_value: ident.to_string(),
        actual_memberkey: None,
        actual_value: Value::Null,
      }
      .into(),
    ),
  }
}

/// Validates JSON input against given CDDL input
pub fn validate_json_from_str(cddl_input: &str, json_input: &str) -> Result {
  validate_json(
    &parser::cddl_from_str(&mut lexer::Lexer::new(cddl_input), cddl_input, false)
      .map_err(|e| Error::Compilation(CompilationError::CDDL(e)))?,
    &serde_json::from_str(json_input)
      .map_err(|e| Error::Compilation(CompilationError::Target(e.into())))?,
  )
}

fn validate_json<V: Validator<Value>>(cddl: &V, json: &Value) -> Result {
  cddl.validate(json)
}

fn is_type_json_prelude(t: &str) -> bool {
  match t {
    "any" | "uint" | "nint" | "int" | "tstr" | "text" | "number" | "float16" | "float32"
    | "float64" | "float16-32" | "float32-64" | "float" | "false" | "true" | "bool" | "nil"
    | "null" => true,
    _ => false,
  }
}

fn validate_tdate(value: &str) -> Result {
  let _ = chrono::DateTime::parse_from_rfc3339(value)
    .map_err(|e| Error::Syntax(format!("Error parsing date value {}: {}", value, e)))?;

  Ok(())
}

#[cfg(feature = "nightly")]
fn validate_uri(value: &str) -> Result {
  let _ = uriparse::uri_reference::URIReference::try_from(value)
    .map_err(|e| Error::Syntax(format!("Error parsing URI value {}: {}", value, e)))?;

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn validate_json_null() -> Result {
    let json_input = r#"null"#;

    let cddl_input = r#"mynullrule = null"#;

    validate_json_from_str(cddl_input, json_input)
  }

  #[test]
  fn validate_json_bool() -> Result {
    let json_input = r#"true"#;

    let cddl_input = r#"myboolrule = true"#;

    validate_json_from_str(cddl_input, json_input)
  }

  #[test]
  fn validate_json_number() -> Result {
    let json_inputs = [r#"3"#, r#"1.5"#, r#"10"#];

    let cddl_input = r#"mynumericrule = 3 / 1.5 / 10"#;

    for ji in json_inputs.iter() {
      validate_json_from_str(cddl_input, ji)?;
    }

    Ok(())
  }

  #[test]
  fn validate_json_string() -> Result {
    let json_input = r#""mystring""#;

    let cddl_input = r#"mystringrule = "mystring""#;

    validate_json_from_str(cddl_input, json_input)
  }

  #[test]
  fn validate_json_object() -> Result {
    let json_input = r#"{
      "mykey": "myvalue",
      "myarray": [
        {
          "myotherkey": "myothervalue"
        }
      ]
    }"#;

    let cddl_input = r#"myobject = {
      mykey: tstr,
      myarray: [1* arraytype],
    }
    
    arraytype = {
      myotherkey: tstr,
    }"#;

    if let Err(Error::Compilation(e)) = validate_json_from_str(cddl_input, json_input) {
      println!("{}", e);
    }

    Ok(())
  }

  #[test]
  fn validate_json_array() -> Result {
    let json_input = r#"[
      "washington",
      {
        "longitude": 1234,
        "latitude": 3947
      }
    ]"#;

    let cddl_input = r#"Geography = [
      city           : tstr,
      gpsCoordinates : GpsCoordinates,
    ]

    GpsCoordinates = {
      longitude      : uint,            ; degrees, scaled by 10^7
      latitude       : uint,            ; degrees, scaled by 10^7
    }"#;

    validate_json_from_str(cddl_input, json_input)
  }

  #[test]
  fn validate_with_group_enum() -> Result {
    let json_input = r#""blue""#;

    let cddl_input = r#"color = &colors

    colors = (
      red: "red", blue: "blue", green: "green",
    )"#;

    validate_json_from_str(cddl_input, json_input)
  }

  #[test]
  fn validate_number_int_range() -> Result {
    let json_input = r#"3"#;
    let cddl_input = r#"myrange = my.lower .. upper
    
    my.lower = -1
    upper = 1 / 3"#;

    validate_json_from_str(cddl_input, json_input)
  }

  #[test]
  fn validate_cut_in_map() -> Result {
    let json_input = r#"{ "optional-key": 10 }"#;
    let cddl_input = r#"extensible-map-example = {
      ? "optional-key" ^ => int,
      * tstr => any
    }"#;

    validate_json_from_str(cddl_input, json_input)
  }

  #[test]
  fn validate_bad_range() -> Result {
    let json_input = r#"3"#;
    let cddl_input = r#"badrange = 1.5...4"#;

    assert!(validate_json_from_str(cddl_input, json_input).is_err());

    Ok(())
  }

  #[test]
  fn validate_uri_text_value() -> Result {
    let json_input = r#""https://gitub.com""#;
    let cddl_input = r#"root = uri"#;

    validate_json_from_str(cddl_input, json_input)
  }
}
