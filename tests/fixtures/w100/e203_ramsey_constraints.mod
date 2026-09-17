// inventory: e203_ramsey_constraints
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
ramsey_constraints;
y > 0;
end;
