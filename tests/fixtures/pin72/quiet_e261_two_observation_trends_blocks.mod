// inventory: pin72_quiet_e261_two_observation_trends_blocks
var y z;
varexo e e2;
parameters rho;
rho = .5;
model;
y = rho*y(-1) + e + e2;
z = y;
end;
varobs y;
observation_trends;
y(1);
end;
observation_trends;
y(2);
end;
