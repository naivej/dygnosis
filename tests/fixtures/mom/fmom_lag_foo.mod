// inventory: fmom_lag_foo
// lag that is a name
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
moment_calibration;
y, c(foo), [0, 1];
end;
