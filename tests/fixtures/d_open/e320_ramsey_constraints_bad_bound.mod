// inventory: e320_ramsey_constraints_bad_bound
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
ramsey_model;
ramsey_constraints;
y > e;
end;
