import test from 'node:test';
import assert from 'node:assert/strict';
import { average, positiveValues, positiveTotal } from './stats.js';

test('empty average is zero', () => assert.equal(average([]), 0));
test('positive values and totals agree', () => {
  assert.deepEqual(positiveValues([-1, 0, 2, '3', 4]), [2, 4]);
  assert.equal(positiveTotal([-1, 0, 2, '3', 4]), 6);
});
test('average ignores non-numeric entries in the denominator', () => {
  assert.equal(average([2, '3', 4]), 3);
  assert.equal(average([1, null, 3]), 2);
});
test('average handles a single value and negatives', () => {
  assert.equal(average([5]), 5);
  assert.equal(average([-2, -4]), -3);
});
test('average ignores non-finite values and returns zero when none remain', () => {
  assert.equal(average([2, Infinity, 4]), 3);
  assert.equal(average([NaN, Infinity]), 0);
});
test('positiveValues excludes zero, negatives, non-numbers and non-finite values', () => {
  assert.deepEqual(positiveValues([-0, 0, -1, NaN, Infinity, 1]), [1]);
});
test('positiveValues ignores Infinity and NaN', () => {
  assert.deepEqual(positiveValues([Infinity, -Infinity, NaN, 2, 3]), [2, 3]);
});
test('positiveTotal ignores Infinity and NaN', () => {
  assert.equal(positiveTotal([Infinity, -Infinity, NaN, 2, 3]), 5);
});
test('positiveTotal equals the sum of positiveValues', () => {
  const input = [-1, 0, 2, '3', 4, NaN, 10];
  assert.equal(
    positiveTotal(input),
    positiveValues(input).reduce((sum, value) => sum + value, 0),
  );
});
