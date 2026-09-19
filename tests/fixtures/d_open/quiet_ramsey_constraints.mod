// inventory: quiet_ramsey_constraints
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
ramsey_model;
ramsey_constraints;
0 < y < 1;
end;
