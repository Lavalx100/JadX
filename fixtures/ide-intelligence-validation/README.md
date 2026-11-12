# JadX IDE intelligence validation project

Small, dependency-free Node.js fixtures used to validate JadX IDE analysis and reviewed patch workflows. No private data is included.

`average(values)` averages finite numbers and returns `0` when none remain. `positiveValues(values)` keeps values whose JavaScript type is `number`, whose value is greater than zero, and which are finite; positive infinity and NaN are ignored. `positiveTotal(values)` sums that result.

Run `npm test` to execute the behavioral and edge-case coverage with Node's built-in test runner.
