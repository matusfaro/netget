# USB Mouse E2E Testing

## Test Strategy

E2E tests verify virtual HID mouse using Linux `usbip` client and event monitoring tools.

## Test Cases (Planned)

### Test 1: Mouse Movement
- Start server with instruction "Move cursor right 100 pixels"
- Attach device
- Verify movement events with `evtest` or `xinput test`
- **LLM calls**: 1

### Test 2: Mouse Clicks
- Instruction: "Click left button 3 times"
- Verify button events
- **LLM calls**: 1

### Test 3: Scroll Wheel
- Instruction: "Scroll up 5 times"
- Verify scroll events
- **LLM calls**: 1

### Test 4: Drag Operation
- Instruction: "Drag from (100,100) to (200,200)"
- Verify movement + button hold + release
- **LLM calls**: 1

## LLM Budget

**Target**: < 10 calls total

**Strategy**: Use scripting mode for deterministic behavior

## Runtime

**Expected**: < 15 seconds for full suite

## Requirements

- Linux with usbip and evtest
- `sudo modprobe vhci-hcd`
- Root access for device attachment
