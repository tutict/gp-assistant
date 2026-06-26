//! C-ABI FFI surface for gp-core.
//!
//! Every exported function funnels through `ffi_response`, which null-checks and
//! UTF-8-validates the input pointer, catches panics so they never cross the FFI
//! boundary (UB), and returns an owned C string the caller frees via `gp_core_free_string`.
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic;

use serde_json::{json, Value};

use crate::*;

fn c_string_to_str<'a>(input: *const c_char) -> CoreResult<&'a str> {
    if input.is_null() {
        return Err(CoreError::new("Input pointer is null"));
    }
    unsafe { CStr::from_ptr(input) }
        .to_str()
        .map_err(|error| CoreError::new(error.to_string()))
}

fn ffi_response<F>(input: *const c_char, handler: F) -> *mut c_char
where
    F: FnOnce(&str) -> CoreResult<Value> + panic::UnwindSafe,
{
    let result = panic::catch_unwind(|| {
        let input = c_string_to_str(input)?;
        handler(input)
    });

    let envelope = match result {
        Ok(Ok(data)) => json!({ "ok": true, "data": data }),
        Ok(Err(error)) => json!({ "ok": false, "error": error.to_string() }),
        Err(_) => json!({ "ok": false, "error": "Rust core panicked" }),
    };

    let output = envelope.to_string().replace('\0', "");
    CString::new(output)
        .expect("JSON output should not contain null bytes")
        .into_raw()
}

#[no_mangle]
pub extern "C" fn gp_core_screen_json(criteria_json: *const c_char) -> *mut c_char {
    ffi_response(criteria_json, |input| {
        let criteria: ScreenCriteria = serde_json::from_str(input)?;
        serde_json::to_value(screen_with_mock(&criteria)).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_screen_with_data_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: ScreenWithDataRequest = serde_json::from_str(input)?;
        serde_json::to_value(screen_with_data(&request.data, &request.criteria)?)
            .map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_graph_screen_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: GraphScreenRequest = serde_json::from_str(input)?;
        serde_json::to_value(graph_screen_with_mock(&request)).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_graph_screen_with_data_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: GraphScreenWithDataRequest = serde_json::from_str(input)?;
        serde_json::to_value(graph_screen_with_data(&request.data, &request.request)?)
            .map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_backtest_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: BacktestRequest = serde_json::from_str(input)?;
        serde_json::to_value(backtest_with_mock(&request)?).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_backtest_with_data_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: BacktestWithDataRequest = serde_json::from_str(input)?;
        serde_json::to_value(backtest_with_data(&request.data, &request.request)?)
            .map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_trend_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: TrendIndicatorRequest = serde_json::from_str(input)?;
        serde_json::to_value(trend_with_mock(&request)?).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_trend_with_data_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: TrendWithDataRequest = serde_json::from_str(input)?;
        serde_json::to_value(trend_with_data(&request.data, &request.request)?).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_trend_screen_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: TrendScreenRequest = serde_json::from_str(input)?;
        serde_json::to_value(trend_screen_with_mock(&request)?).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_trend_screen_with_data_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: TrendScreenWithDataRequest = serde_json::from_str(input)?;
        serde_json::to_value(trend_screen_with_data(&request.data, &request.request)?)
            .map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_agent_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: AgentRequest = serde_json::from_str(input)?;
        serde_json::to_value(run_agent_with_mock(&request.message)?).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_agent_with_data_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: AgentWithDataRequest = serde_json::from_str(input)?;
        serde_json::to_value(run_agent_with_data(&request.data, &request.message)?)
            .map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_mobile_stock_skill_json(request_json: *const c_char) -> *mut c_char {
    ffi_response(request_json, |input| {
        let request: MobileStockSkillRequest = serde_json::from_str(input)?;
        serde_json::to_value(run_mobile_stock_skill(&request)).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_validate_data_source_json(data_json: *const c_char) -> *mut c_char {
    ffi_response(data_json, |input| {
        let data: CoreDataSet = serde_json::from_str(input)?;
        serde_json::to_value(validate_data_set(&data)?).map_err(Into::into)
    })
}

#[no_mangle]
pub extern "C" fn gp_core_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}
