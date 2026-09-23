// inventory: fmom_irf_minus
// negative irf period
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
irf_calibration;
y(-1), e, [0, 1];
end;
