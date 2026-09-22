// inventory: mom_e387_ic_bad_shock
// `irf_calibration` row whose shock is an endogenous. 7.1's sentence names the
// row's endogenous, not the shock: `Variable y is not an exogenous.`
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

irf_calibration;
y, y, [0, 1];
end;
