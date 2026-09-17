// inventory: e178_surprise
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
shocks(surprise);
var e;
periods 1;
values 0.01;
end;
