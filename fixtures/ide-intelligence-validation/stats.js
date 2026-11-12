// A small executable fixture with a deliberate edge-case bug for IDE validation.
export function average(values) {
  const finite = values.filter((value) => typeof value === 'number' && Number.isFinite(value));
  if (finite.length === 0) return 0;
  return finite.reduce((sum, value) => sum + value, 0) / finite.length;
}

export function positiveValues(values) {
  const result = [];
  for (const value of values) {
    if (typeof value === 'number' && Number.isFinite(value) && value > 0) result.push(value);
  }
  return result;
}

export function positiveTotal(values) {
  return positiveValues(values).reduce((sum, value) => sum + value, 0);
}
