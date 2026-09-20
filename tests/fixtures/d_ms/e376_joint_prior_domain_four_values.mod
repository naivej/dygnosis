// inventory: e376_joint_prior_domain_four_values
// 7.1 refuses this file: `You must pass exactly four values to the domain option.`
var R Pie Y;
varexo eps;
parameters alpha beta;
alpha = 0.36;
beta = 0.99;
model;
R = beta*R(-1) + eps;
Pie = alpha*R(-1) + eps;
Y = beta*Pie(-1) + eps;
end;
initval;
R = 0;
Pie = 0;
Y = 0;
end;
shocks;
var eps; stderr 0.1;
end;
varobs Y Pie R;
[alpha, beta].prior(shape=inv_gamma, mean=[0.5, 0.6], stdev=1.0, domain=[0.1 0.2 0.3]);
