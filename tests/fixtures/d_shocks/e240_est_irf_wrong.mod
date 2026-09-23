var y;
varexo e u;
parameters p;
p=1;
model;
y=e+u+p;
end;
estimation(datafile='data.mat', irf_shocks=(y));
