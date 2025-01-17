extern crate proc_macro;

use std::io::Error as IOError;
use byteorder::{ByteOrder, WriteBytesExt, BE, LE};
use failure::Fail;
use quote::{ToTokens, quote};
use proc_macro::TokenStream;
use syn::{parse_macro_input, Error as SynError, Expr, IntSuffix, FloatSuffix, Lit, LitInt, LitFloat, Token, UnOp};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endianness {
    LE,
    BE,
}

#[cfg(not(feature = "default-big-endian"))]
const DEFAULT_ENDIANNESS: Endianness = Endianness::LE;

#[cfg(feature = "default-big-endian")]
const DEFAULT_ENDIANNESS: Endianness = Endianness::BE;

#[derive(Debug, Fail)]
enum Error {
    #[fail(display = "Unsupported prefixed expression in the macro: {} [+] {}", _0, _1)]
    UnsupportedPrefixedExpression(String, String),
    #[fail(display = "Unsupported expression in the macro: {}", _0)]
    UnsupportedExpression(String),
    #[fail(display = "Unsupported literal in the macro: {}", _0)]
    UnsupportedLit(String),
    #[fail(display = "Unsupported numeric suffix in the macro: {}", _0)]
    UnsupportedNumberSuffix(String),
    #[fail(display = "Failed to parse the input as a comma-separated list: {}", _0)]
    InvalidInput(#[cause] SynError),
    #[fail(display = "Failed to parse endianness: {}", _0)]
    InvalidEndianness(String),
    #[fail(display = "Failed to write a suffixed value: {}, negative: {}, given suffix: {}, requested suffix: {}", _0, _1, _2, _3)]
    IncompatibleNumberSuffix(String, bool, String, String),
    #[fail(display = "Failed to write a value: {}", _0)]
    IO(#[cause] IOError),
}

impl From<SynError> for Error {

    fn from(from: SynError) -> Self {
        Error::InvalidInput(from)
    }
}

impl From<IOError> for Error {

    fn from(from: IOError) -> Self {
        Error::IO(from)
    }
}

impl Error {

    pub fn unsupported_expression(expr: Expr) -> Self {
        Error::UnsupportedExpression(expr.into_token_stream().to_string())
    }

    pub fn unsupported_lit(lit: Lit) -> Self {
        Error::UnsupportedLit(lit.into_token_stream().to_string())
    }

    pub fn unsupported_prefixed_expression(op: UnOp, expr: Expr) -> Self {
        Error::UnsupportedPrefixedExpression(op.into_token_stream().to_string(), expr.into_token_stream().to_string())
    }
}

fn int_to_suffix(negative: bool, int: &LitInt) -> Result<IntSuffix, Error> {
    let num_bits = int.value();
    let s = if negative {
        match () {
            () if num_bits > 0x80000000 => IntSuffix::I64,
            () if num_bits > 0x8000     => IntSuffix::I32,
            () if num_bits > 0x80       => IntSuffix::I16,
            () => IntSuffix::I8,
        }
    } else {
        match () {
            () if num_bits > 0xFFFFFFFF => IntSuffix::U64,
            () if num_bits > 0xFFFF     => IntSuffix::U32,
            () if num_bits > 0xFF       => IntSuffix::U16,
            () => IntSuffix::U8,
        }
    };
    let s = match (s, int.suffix()) {
        // If none is specified use the least size suffix possible.
        (s, IntSuffix::None) => s,
        // Allowed casts Uint -> Uint.
        (IntSuffix::U8 , IntSuffix::U8 ) => IntSuffix::U8 ,
        (IntSuffix::U8 , IntSuffix::U16) => IntSuffix::U16,
        (IntSuffix::U8 , IntSuffix::U32) => IntSuffix::U32,
        (IntSuffix::U8 , IntSuffix::U64) => IntSuffix::U64,
        (IntSuffix::U16, IntSuffix::U16) => IntSuffix::U16,
        (IntSuffix::U16, IntSuffix::U32) => IntSuffix::U32,
        (IntSuffix::U16, IntSuffix::U64) => IntSuffix::U64,
        (IntSuffix::U32, IntSuffix::U32) => IntSuffix::U32,
        (IntSuffix::U32, IntSuffix::U64) => IntSuffix::U64,
        (IntSuffix::U64, IntSuffix::U64) => IntSuffix::U64,
        // Allowed casts Sint -> Sint.
        (IntSuffix::I8 , IntSuffix::I8 ) => IntSuffix::I8 ,
        (IntSuffix::I8 , IntSuffix::I16) => IntSuffix::I16,
        (IntSuffix::I8 , IntSuffix::I32) => IntSuffix::I32,
        (IntSuffix::I8 , IntSuffix::I64) => IntSuffix::I64,
        (IntSuffix::I16, IntSuffix::I16) => IntSuffix::I16,
        (IntSuffix::I16, IntSuffix::I32) => IntSuffix::I32,
        (IntSuffix::I16, IntSuffix::I64) => IntSuffix::I64,
        (IntSuffix::I32, IntSuffix::I32) => IntSuffix::I32,
        (IntSuffix::I32, IntSuffix::I64) => IntSuffix::I64,
        (IntSuffix::I64, IntSuffix::I64) => IntSuffix::I64,
        // Allowed casts Uint -> Sint.
        (IntSuffix::U8 , IntSuffix::I8 ) if num_bits < 0x80               => IntSuffix::I8 ,
        (IntSuffix::U16, IntSuffix::I16) if num_bits < 0x8000             => IntSuffix::I16,
        (IntSuffix::U32, IntSuffix::I32) if num_bits < 0x80000000         => IntSuffix::I32,
        (IntSuffix::U64, IntSuffix::I64) if num_bits < 0x8000000000000000 => IntSuffix::I64,
        (IntSuffix::U8 , IntSuffix::I16) => IntSuffix::I16,
        (IntSuffix::U8 , IntSuffix::I32) => IntSuffix::I32,
        (IntSuffix::U8 , IntSuffix::I64) => IntSuffix::I64,
        (IntSuffix::U16, IntSuffix::I32) => IntSuffix::I32,
        (IntSuffix::U16, IntSuffix::I64) => IntSuffix::I64,
        (IntSuffix::U32, IntSuffix::I64) => IntSuffix::I64,
        // Everything else is either invalid or ambiguous.
        (given, requested) => {
            return Err(Error::IncompatibleNumberSuffix(
                int.into_token_stream().to_string(),
                negative,
                format!("{:?}", given),
                format!("{:?}", requested),
            ));
        },
    };
    Ok(s)
}

fn bytify_implementation_int<O: ByteOrder>(negative: bool, int: LitInt, output: &mut Vec<u8>) -> Result<(), Error> {
    let num_bits = int.value();
    let num_bits_suffix = int_to_suffix(negative, &int)?;
    match num_bits_suffix {
        IntSuffix::U8 => {
            output.write_u8(num_bits as u8)?;
        },
        IntSuffix::I8 => {
            if negative {
                output.write_u8((!(num_bits as u8)).wrapping_add(1))?;
            } else {
                output.write_u8(   num_bits as u8)?;
            }
        },
        IntSuffix::U16 => {
            output.write_u16::<O>(num_bits as u16)?;
        },
        IntSuffix::I16 => {
            if negative {
                output.write_u16::<O>((!(num_bits as u16)).wrapping_add(1))?;
            } else {
                output.write_u16::<O>(   num_bits as u16)?;
            }
        },
        IntSuffix::U32 => {
            output.write_u32::<O>(num_bits as u32)?;
        },
        IntSuffix::I32 => {
            if negative {
                output.write_u32::<O>((!(num_bits as u32)).wrapping_add(1))?;
            } else {
                output.write_u32::<O>(   num_bits as u32)?;
            }
        },
        IntSuffix::U64 => {
            output.write_u64::<O>(num_bits as u64)?;
        },
        IntSuffix::I64 => {
            if negative {
                output.write_u64::<O>((!(num_bits as u64)).wrapping_add(1))?;
            } else {
                output.write_u64::<O>(   num_bits as u64)?;
            }
        },
        // Everything else is either invalid or ambiguous.
        s => {
            return Err(Error::UnsupportedNumberSuffix(format!("{:?}", s)));
        },
    }
    Ok(())
}

fn float_to_suffix(negative: bool, float: &LitFloat) -> Result<FloatSuffix, Error> {
    let num_bits = float.value();
    let s = if num_bits > 3.40282347e+38 {
        FloatSuffix::F64
    } else {
        FloatSuffix::F32
    };
    let s = match (s, float.suffix()) {
        // If none is specified use the least size suffix possible.
        (s, FloatSuffix::None) => s,
        // The only possible float cast.
        (FloatSuffix::F32, FloatSuffix::F64) => FloatSuffix::F64,
        // Everything else is either invalid or ambiguous.
        (given, requested) => {
            return Err(Error::IncompatibleNumberSuffix(
                float.into_token_stream().to_string(),
                negative,
                format!("{:?}", given),
                format!("{:?}", requested),
            ));
        },
    };
    Ok(s)
}

fn bytify_implementation_float<O: ByteOrder>(negative: bool, float: LitFloat, output: &mut Vec<u8>) -> Result<(), Error> {
    let num_bits = float.value();
    let num_bits_suffix = float_to_suffix(negative, &float)?;
    match num_bits_suffix {
        FloatSuffix::F32 => {
            if negative {
                output.write_f32::<O>(-(num_bits as f32))?;
            } else {
                output.write_f32::<O>(  num_bits as f32 )?;
            }
        },
        FloatSuffix::F64 => {
            if negative {
                output.write_f64::<O>(-num_bits)?;
            } else {
                output.write_f64::<O>( num_bits)?;
            }
        },
        // Everything else is either invalid or ambiguous.
        s => {
            return Err(Error::UnsupportedNumberSuffix(format!("{:?}", s)));
        },
    }
    Ok(())
}

fn bytify_implementation_element<O: ByteOrder>(lit: Lit, output: &mut Vec<u8>) -> Result<(), Error> {
    match lit {
        Lit::Char(c) => {
            let offset = output.len();
            output.resize(c.value().len_utf8() + offset, 0u8);
            c.value().encode_utf8(&mut output[offset ..]);
        },
        Lit::Str(string) => {
            output.extend_from_slice(string.value().as_bytes());
        },
        Lit::Int(int) => {
            bytify_implementation_int::<O>(false, int, output)?;
        },
        Lit::Float(float) => {
            bytify_implementation_float::<O>(false, float, output)?;
        },
        lit => {
            return Err(Error::unsupported_lit(lit));
        },
    }
    Ok(())
}

#[derive(Debug)]
struct MyMacroInput {
    list: Punctuated<Expr, Token![,]>,
}

impl Parse for MyMacroInput {

    fn parse(input: ParseStream) -> Result<Self, SynError> {
        Ok(MyMacroInput {
            list: input.parse_terminated(Expr::parse)?,
        })
    }
}

fn bytify_implementation(input: MyMacroInput) -> Result<TokenStream, Error> {
    let mut output: Vec<u8> = Vec::new();
    for expr in input.list {
        let (
            endianness,
            expr,
        ) = match expr {
            /* it is not, actually! */ Expr::Type(tpe_expr) => {
                let expr = *tpe_expr.expr;
                let endianness = match tpe_expr.ty.into_token_stream().to_string().as_str() {
                    "BE" | "be" => Endianness::BE,
                    "LE" | "le" => Endianness::LE,
                    invalid => {
                        return Err(Error::InvalidEndianness(invalid.to_string()));
                    },
                };
                (endianness, expr)
            },
            expr => {
                (DEFAULT_ENDIANNESS, expr)
            },
        };
        match expr {
            Expr::Lit(lit_expr) => {
                if endianness == Endianness::BE {
                    bytify_implementation_element::<BE>(lit_expr.lit, &mut output)?;
                } else {
                    bytify_implementation_element::<LE>(lit_expr.lit, &mut output)?;
                }
            },
            Expr::Unary(unary_expr) => {
                match unary_expr.op {
                    UnOp::Neg(op) => {
                        match *unary_expr.expr {
                            Expr::Lit(lit_expr) => {
                                match lit_expr.lit {
                                    Lit::Int(int) => {
                                        if endianness == Endianness::BE {
                                            bytify_implementation_int::<BE>(true, int, &mut output)?;
                                        } else {
                                            bytify_implementation_int::<LE>(true, int, &mut output)?;
                                        }
                                    },
                                    Lit::Float(float) => {
                                        if endianness == Endianness::BE {
                                            bytify_implementation_float::<BE>(true, float, &mut output)?;
                                        } else {
                                            bytify_implementation_float::<LE>(true, float, &mut output)?;
                                        }
                                    },
                                    lit => {
                                        return Err(Error::unsupported_lit(lit));
                                    },
                                }
                            },
                            expr => {
                                return Err(Error::unsupported_prefixed_expression(UnOp::Neg(op), expr));
                            },
                        }
                    },
                    op => {
                        return Err(Error::unsupported_prefixed_expression(op, *unary_expr.expr));
                    },
                }
            },
            expr => {
                return Err(Error::unsupported_expression(expr));
            },
        }
    }
    Ok(quote! {
        [
            #(#output),*
        ]
    }.into())
}

#[proc_macro]
pub fn bytify(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as MyMacroInput);
    bytify_implementation(input).unwrap_or_else(|err| panic!("{}", err))
}
