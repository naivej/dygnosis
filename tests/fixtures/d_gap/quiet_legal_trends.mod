// inventory: quiet_legal_trends
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
trend_var(growth_factor=1.02) A;
var(deflator=A) z;
epilogue;
foo = 1;
bar = 2;
end;
