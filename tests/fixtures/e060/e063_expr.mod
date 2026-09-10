// inventory: e063_expr_no_include
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + @{UNDEF+1};
end;
