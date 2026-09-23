// inventory: fmom_kw_miss_periods
// values without periods
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_irfs;
var y; varexo e; values 1;
end;
