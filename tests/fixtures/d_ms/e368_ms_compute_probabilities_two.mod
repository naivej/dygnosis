// inventory: e368_ms_compute_probabilities_two
// 7.1 refuses this file: `You may only pass one of real_time_smoothed and filtered_probabilities to ms_compute_probabilities.`
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
markov_switching(chain=1, number_of_regimes=2, duration=2.5);
ms_compute_probabilities(real_time_smoothed, filtered_probabilities);
