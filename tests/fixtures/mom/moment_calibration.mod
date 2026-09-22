// inventory: mom_moment_calibration_rows
// Original AR(1) plus a definition. Three `moment_calibration` rows: a bracket
// with no lags, `+` with one lag, and `-` with a signed lag range.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

moment_calibration;
y, c, [0.5, 1.2];
y, y(1), +;
y, c(-2:2), -;
end;
