// inventory: mom_e317_mirf_det_shock
// `matched_irfs` row whose `varexo` is a `varexo_det`. 7.1 refuses its second
// E317 sentence: `ed is an exogenous deterministic.`
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

matched_irfs;
var y; varexo ed; periods 1; values 1;
end;
