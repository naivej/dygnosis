// inventory: fmom_mcal_extra
// paren on moment_calibration
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
moment_calibration(extra);
y, c, [0, 1];
end;
