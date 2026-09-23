// inventory: fmom_lag_float
// fractional lag
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
moment_calibration;
y, c(1.5), [0, 1];
end;
