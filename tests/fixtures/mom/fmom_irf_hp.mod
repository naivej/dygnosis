// inventory: fmom_irf_hp
// option irf_calibration does not take
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
irf_calibration(hp_filter=1600);
y, e, [0, 1];
end;
