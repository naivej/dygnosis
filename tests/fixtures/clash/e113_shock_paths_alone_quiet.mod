// inventory: e113_shock_paths_alone_quiet
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
shock_paths;
var e;
periods 1;
values 0;
end;
