// inventory: mom_e317_mc_not_endogenous
// `moment_calibration` row whose first name is a shock. 7.1 refuses:
// `e is not endogenous.`
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

moment_calibration;
e, y, [0, 1];
end;
