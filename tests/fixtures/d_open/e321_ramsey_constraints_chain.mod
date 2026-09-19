// inventory: e321_ramsey_constraints_chain
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
ramsey_model;
ramsey_constraints;
y > 0 > -1;
end;
