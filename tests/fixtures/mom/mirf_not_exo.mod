// inventory: mom_e387_mirf_not_exogenous
// `matched_irfs` row whose shock is an endogenous. 7.1 refuses:
// `y is not exogenous.`
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

matched_irfs;
var y; varexo y; periods 1; values 1;
end;
