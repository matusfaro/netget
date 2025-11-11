# USB Serial E2E Testing

## Test Strategy

E2E tests verify virtual CDC ACM serial using Linux `usbip` client and `/dev/ttyACM0`.

## Test Cases (Planned)

### Test 1: Echo Server

- Start server with instruction "Echo back any data received"
- Attach device, appears as `/dev/ttyACM0`
- Send "test" → verify "test" echoed back
- **LLM calls**: 2 (attach + receive)

### Test 2: Line Coding

- Change baud rate to 9600
- Verify configuration accepted
- **LLM calls**: 1

### Test 3: Bidirectional

- LLM sends periodic data
- Verify host receives data
- **LLM calls**: 3-5

## LLM Budget: < 10 calls

## Runtime: < 15 seconds

## Tools

- `screen /dev/ttyACM0 115200` or `minicom`
- `cat /dev/ttyACM0` and `echo "test" > /dev/ttyACM0`
