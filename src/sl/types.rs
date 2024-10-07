const SL_RESULT_OK: u32 = 0;
const SL_RESULT_FAIL_BIT: u32 = 0x80000000;
const SL_RESULT_ALREADY_DONE: u32 = 0x20;
const SL_RESULT_INVALID_DATA: u32 = 0x8000 | SL_RESULT_FAIL_BIT;
const SL_RESULT_OPERATION_FAIL: u32 = 0x8001 | SL_RESULT_FAIL_BIT;
const SL_RESULT_OPERATION_TIMEOUT: u32 = 0x8002 | SL_RESULT_FAIL_BIT;
const SL_RESULT_OPERATION_STOP: u32 = 0x8003 | SL_RESULT_FAIL_BIT;
const SL_RESULT_OPERATION_NOT_SUPPORT: u32 = 0x8004 | SL_RESULT_FAIL_BIT;
const SL_RESULT_FORMAT_NOT_SUPPORT: u32 = 0x8005 | SL_RESULT_FAIL_BIT;
const SL_RESULT_INSUFFICIENT_MEMORY: u32 = 0x8006 | SL_RESULT_FAIL_BIT;

const fn sl_is_ok(x: u32) -> bool {
    (x & SL_RESULT_FAIL_BIT) == 0
}

const fn sl_is_err(x: u32) -> bool {
    (x & SL_RESULT_FAIL_BIT) != 0
}