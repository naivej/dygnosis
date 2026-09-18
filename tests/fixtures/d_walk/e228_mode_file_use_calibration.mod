// inventory: e228_mode_file_use_calibration
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
estimated_params_init(use_calibration);
end;
estimation(mode_file='m.mat', datafile='d.csv');
