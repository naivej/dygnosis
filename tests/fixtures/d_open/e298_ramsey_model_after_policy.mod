// inventory: e298_ramsey_model_after_policy
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
ramsey_policy;
ramsey_model;
