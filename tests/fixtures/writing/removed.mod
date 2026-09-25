var y (long_name='output'), x (long_name='other');
model;
[name='drop'] y = 5;
[name='keep'] x = y(-1);
end;
model_remove('drop');
