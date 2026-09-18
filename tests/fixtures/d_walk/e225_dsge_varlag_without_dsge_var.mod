// inventory: e225_dsge_varlag_without_dsge_var
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
estimation(dsge_varlag=4);
