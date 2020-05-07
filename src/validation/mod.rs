/// CBOR validation implementation
pub mod cbor;

/// JSON validation implementation
pub mod json;

use crate::{ast::*, token::Numeric};
use std::{fmt, result};

/// Alias for `Result` with an error of type `validator::ValidationError`
pub type Result = result::Result<(), Error>;

/// Validation error types
#[derive(Debug)]
pub enum Error {
  /// CDDL syntax error, specific to the target data structure being validated
  Syntax(String),
  /// Error validating specific target data structure (i.e. JSON or CBOR)
  Target(Box<dyn std::error::Error>),
  /// Error compiling CDDL and/or target data structure
  Compilation(CompilationError),
  /// Occurrence error
  Occurrence(String),
  /// Aggregate errors
  MultiError(Vec<Error>),
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      Error::Syntax(se) => write!(f, "CDDL syntax error: {}", se),
      Error::Target(te) => write!(f, "{}", te),
      Error::Compilation(ce) => write!(f, "error on compilation: {}", ce),
      Error::Occurrence(oe) => write!(f, "occurrence error: {}", oe),
      Error::MultiError(me) => {
        let mut errors = String::new();

        for e in me.iter() {
          match e {
            // Temporary work around for nested MultiError's
            Error::MultiError(_) => errors.push_str(&format!("{}", e)),
            _ => errors.push_str(&format!("{}\n\n", e)),
          }
        }

        write!(f, "{}", errors)
      }
    }
  }
}

impl std::error::Error for Error {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    match self {
      Error::Compilation(ce) => Some(ce),
      Error::Target(te) => Some(te.as_ref()),
      _ => None,
    }
  }
}

/// Compilation errors
#[derive(Debug)]
pub enum CompilationError {
  /// Error compiling CDDL data definition
  CDDL(String),
  /// Error compiling data target (i.e. JSON or CBOR)
  Target(Box<dyn std::error::Error>),
}

impl fmt::Display for CompilationError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      CompilationError::CDDL(ce) => write!(f, "{}", ce),
      CompilationError::Target(te) => write!(f, "{}", te),
    }
  }
}

impl std::error::Error for CompilationError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    match self {
      CompilationError::Target(te) => Some(te.as_ref()),
      _ => None,
    }
  }
}

/// Required behavior of a validator over different data types
pub trait Validator<T> {
  /// Initiate validation
  fn validate(&self, value: &T) -> Result;

  /// Validate data against the rule with the given identifier
  fn validate_rule_for_ident(
    &self,
    ident: &Identifier,
    is_enumeration: bool,
    expected_memberkey: Option<String>,
    actual_memberkey: Option<String>,
    occur: Option<&Occurrence>,
    value: &T,
  ) -> Result;

  /// Validate data against a given type rule
  fn validate_type_rule(
    &self,
    tr: &TypeRule,
    expected_memberkey: Option<String>,
    actual_memberkey: Option<String>,
    occur: Option<&Occurrence>,
    value: &T,
  ) -> Result;

  /// Validate data against a given group rule
  fn validate_group_rule(
    &self,
    gr: &GroupRule,
    is_enumeration: bool,
    occur: Option<&Occurrence>,
    value: &T,
  ) -> Result;

  /// Validate data against a given type, which may include additional type
  /// choices
  fn validate_type(
    &self,
    t: &Type,
    expected_memberkey: Option<String>,
    actual_memberkey: Option<String>,
    occur: Option<&Occurrence>,
    value: &T,
  ) -> Result;

  /// Validate data against a given type, which may include an optional range or
  /// control operator
  fn validate_type1(
    &self,
    t1: &Type1,
    expected_memberkey: Option<String>,
    actual_memberkey: Option<String>,
    occur: Option<&Occurrence>,
    value: &T,
  ) -> Result;

  /// Validate data against a given range operation between a lower and upper
  /// bound
  fn validate_range(&self, lower: &Type2, upper: &Type2, is_inclusive: bool, value: &T) -> Result;

  /// Validate data against a given control operator defined by a target type
  /// and a controller types
  fn validate_control_operator(
    &self,
    target: &Type2,
    operator: &str,
    controller: &Type2,
    value: &T,
  ) -> Result;

  /// Validate data against a given type
  fn validate_type2(
    &self,
    t2: &Type2,
    expected_memberkey: Option<String>,
    actual_memberkey: Option<String>,
    occur: Option<&Occurrence>,
    value: &T,
  ) -> Result;

  /// Validate data against a given group
  fn validate_group(&self, g: &Group, occur: Option<&Occurrence>, value: &T) -> Result;

  /// Validate data against a given group-to-choice enumeration
  fn validate_group_to_choice_enum(
    &self,
    g: &Group,
    occur: Option<&Occurrence>,
    value: &T,
  ) -> Result;

  /// Validate data against a given group choice
  fn validate_group_choice(
    &self,
    gc: &GroupChoice,
    occur: Option<&Occurrence>,
    value: &T,
  ) -> Result;

  /// Validate data against a given group entry
  fn validate_group_entry(
    &self,
    ge: &GroupEntry,
    is_enumeration: bool,
    wildcard_entry: Option<&Type>,
    occur: Option<&Occurrence>,
    value: &T,
  ) -> Result;

  /// Validate data against a given array and occurrence indicator
  fn validate_array_occurrence(&self, occur: &Occur, group: &str, values: &[T]) -> Result;

  /// Expect data to be a boolean
  fn expect_bool(&self, ident: &str, value: &T) -> Result;

  /// Validate data against a given numeric data type
  fn validate_numeric_data_type(
    &self,
    expected_memberkey: Option<String>,
    actual_memberkey: Option<String>,
    ident: &str,
    value: &T,
  ) -> Result;
}

impl<'a> CDDL<'a> {
  fn numerical_value_type_from_ident(&self, ident: &Identifier) -> Option<Vec<&Type2>> {
    let mut type_choices = Vec::new();

    for rule in self.rules.iter() {
      match rule {
        Rule::Type { rule, .. } if rule.name.ident == ident.ident => {
          for tc in rule.value.type_choices.iter() {
            match &tc.type1.type2 {
              Type2::IntValue { .. } | Type2::UintValue { .. } | Type2::FloatValue { .. } => {
                type_choices.push(&tc.type1.type2);
              }
              Type2::Typename { ident, .. } => return self.numerical_value_type_from_ident(ident),
              _ => continue,
            }
          }
        }
        _ => continue,
      }
    }

    if !type_choices.is_empty() {
      return Some(type_choices);
    }

    None
  }

  // Checks whether or not a given type is a type name identifier and that it
  // resolves to a text string data type (text | tstr)
  fn is_type_string_data_type(&self, t2: &Type2) -> bool {
    match t2 {
      Type2::Typename { ident, .. } if ident.ident == "text" || ident.ident == "tstr" => true,
      Type2::Typename { ident, .. } => self.rules.iter().any(|r| match r {
        Rule::Type { rule, .. } if rule.name.ident == ident.ident => rule
          .value
          .type_choices
          .iter()
          .any(|tc| self.is_type_string_data_type(&tc.type1.type2)),
        _ => false,
      }),
      _ => false,
    }
  }

  fn is_type_numeric_data_type(&self, t2: &Type2) -> bool {
    match t2 {
      Type2::Typename { ident, .. } if is_numeric_data_type(&ident.ident) => true,
      Type2::Typename { ident, .. } => self.rules.iter().any(|r| match r {
        Rule::Type { rule, .. } if rule.name.ident == ident.ident => rule
          .value
          .type_choices
          .iter()
          .any(|tc| self.is_type_numeric_data_type(&tc.type1.type2)),
        _ => false,
      }),
      _ => false,
    }
  }

  // Returns the text value(s) from a given type
  fn text_values_from_type(&'a self, ident: &'a Type2) -> result::Result<Vec<&'a str>, Error> {
    match ident {
      Type2::TextValue { value, .. } => Ok(vec![value]),
      Type2::Typename { ident, .. } => {
        let mut text_values = Vec::new();

        for r in self.rules.iter() {
          match r {
            Rule::Type { rule, .. } if rule.name.ident == ident.ident => {
              for tc in rule.value.type_choices.iter() {
                text_values.append(&mut self.text_values_from_type(&tc.type1.type2)?);
              }
            }
            _ => continue,
          }
        }

        Ok(text_values)
      }
      _ => Err(Error::Syntax(
        "Value can only be referenced via another type name identifier".into(),
      )),
    }
  }

  // Returns the numeric value(s) from a given type, provided their types match
  // that of the given target data type
  fn numeric_values_from_type(
    &self,
    target: &Type2,
    ident: &Type2,
  ) -> result::Result<Vec<Numeric>, Error> {
    let target_idents = self.numerical_ident_from_type(target)?;

    match ident {
      Type2::IntValue{ value, .. } => {
        if target_idents.into_iter().any(|ti| ti == "int" || ti == "number") {
          return Ok(vec![Numeric::INT(*value)])
        }

        Err(Error::Syntax("Target data type must be an 'int' or 'number' in order to validate against an integer value".into()))
      }
      Type2::UintValue{ value, .. } => {
        if target_idents.into_iter().any(|ti| ti == "uint" || ti == "number") {
          return Ok(vec![Numeric::UINT(*value)]);
        }

        Err(Error::Syntax("Target data type must be a 'uint' or 'number' in order to validate against an unsigned integer value".into()))
      }
      Type2::FloatValue{ value, .. } => {
        if target_idents.into_iter().any(|ti| match ti {
          "float" | "float16" | "float32" | "float64" | "float16-32" | "float32-64" | "number" => true,
          _ => false,
        }) {
          return Ok(vec![Numeric::FLOAT(*value)]);
        }

        Err(Error::Syntax("Target data type must be a 'float', 'float16', 'float32', 'float64', 'float16-32', 'float32-64' or 'number' in order to validate against an unsigned integer value".into()))
      }
      Type2::Typename{ ident, .. } => {
        let mut numeric_values = Vec::new();

        for r in self.rules.iter() {
          match r {
            Rule::Type{ rule, ..}  if rule.name.ident == ident.ident => {
              for tc in rule.value.type_choices.iter() {
                numeric_values.append(&mut self.numeric_values_from_type(target, &tc.type1.type2)?);
              }
            }
            _ => continue,
          }
        }

        Ok(numeric_values)
      }
      _ => Err(Error::Syntax(
        "Value can only be referenced via another type name identifier whose target type is the type of the value".into(),
      )),
    }
  }

  fn numerical_ident_from_type(&'a self, t2: &'a Type2) -> result::Result<Vec<&'a str>, Error> {
    let mut numeric_type_idents = Vec::new();

    match t2 {
      Type2::Typename { ident, .. } if is_numeric_data_type(&ident.ident) => {
        numeric_type_idents.push(ident.ident);
        Ok(numeric_type_idents)
      }
      Type2::Typename { ident, .. } => {
        for r in self.rules.iter() {
          match r {
            Rule::Type { rule, .. } if rule.name.ident == ident.ident => {
              for tc in rule.value.type_choices.iter() {
                numeric_type_idents.append(&mut self.numerical_ident_from_type(&tc.type1.type2)?);
              }
            }
            _ => continue,
          }
        }

        Ok(numeric_type_idents)
      }
      _ => Err(Error::Syntax(
        "Numeric identifier can only be referenced via another type name identifier".into(),
      )),
    }
  }
}

fn is_numeric_data_type(t: &str) -> bool {
  match t {
    "uint" | "nint" | "int" | "number" | "float" | "float16" | "float32" | "float64"
    | "float16-32" | "float32-64" => true,
    _ => false,
  }
}
