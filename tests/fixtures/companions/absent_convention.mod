// inventory: companions_absent_convention
// Missing run_*.m, _prior_restrictions.m, and ident helper: no those records.
var y;
varexo e;
parameters rho;
rho = 0.5;

model;
y = rho * y(-1) + e;
end;

initval;
y = missing_ident_helper(rho);
end;
