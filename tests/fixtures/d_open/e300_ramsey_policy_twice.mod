// inventory: e300_ramsey_policy_twice
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
ramsey_policy;
ramsey_policy;
