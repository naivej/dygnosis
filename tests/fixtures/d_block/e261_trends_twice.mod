// inventory: e261_trends_twice
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
varobs y;
observation_trends;
y(1);
y(1);
end;
