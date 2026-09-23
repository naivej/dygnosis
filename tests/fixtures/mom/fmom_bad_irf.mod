// inventory: fmom_bad_irf
// stray row beside an irf_calibration row
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
irf_calibration;
y, e, [0, 1];
y;
end;
