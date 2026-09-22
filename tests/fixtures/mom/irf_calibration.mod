// inventory: mom_irf_calibration_rows
// Original AR(1) plus a definition. `(relative_irf)` sets the flag; the row is an
// endogenous with a period range, a shock, and the `+` range.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

irf_calibration(relative_irf);
y(1:4), e, +;
end;
