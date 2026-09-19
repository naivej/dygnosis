// inventory: e319_ramsey_constraints_not_inequality
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
ramsey_model;
ramsey_constraints;
y + 1;
end;
