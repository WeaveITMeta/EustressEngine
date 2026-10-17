// A guard against tests that mix a fixed clock with the real one. `npm run test:clock-shift` runs the whole suite with
// Date.now and new Date() moved a year ahead (or by CLOCK_SHIFT_MS when it is set), so a test that stamps one value from a
// fixed date and another from the real clock fails the day it is written, not weeks later. Dates built from an explicit value
// are untouched.
const shift = process.env.CLOCK_SHIFT_MS === undefined ? 365 * 24 * 3600 * 1000 : Number(process.env.CLOCK_SHIFT_MS);
const RealDate = Date;
const realNow = RealDate.now.bind(RealDate);
class ShiftedDate extends RealDate {
  constructor(...args) {
    if (args.length === 0) super(realNow() + shift);
    else super(...args);
  }
  static now() { return realNow() + shift; }
}
globalThis.Date = ShiftedDate;
