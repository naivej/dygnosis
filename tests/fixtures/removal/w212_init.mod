// inventory: assignment before model_remove of truly excluded x
var y x;
model;
y=0.5*y(-1);
[name='drop'] x=y;
end;
initval; y=1; x=1; end;
model_remove('drop');
