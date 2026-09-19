// inventory: e318_ramsey_constraints_two
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
ramsey_model;
ramsey_constraints;
y > 0;
y < 1;
end;
