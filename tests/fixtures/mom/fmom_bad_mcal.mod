// inventory: fmom_bad_mcal
// stray row beside a moment_calibration row
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
moment_calibration;
y, c, [0, 1];
y;
end;
