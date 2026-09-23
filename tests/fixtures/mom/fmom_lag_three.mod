// inventory: fmom_lag_three
// three-part lag range
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
moment_calibration;
y, c(1:2:3), [0, 1];
end;
