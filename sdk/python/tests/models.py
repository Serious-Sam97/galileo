from django.db import models


class Pet(models.Model):
    name = models.CharField(max_length=50)
    species = models.CharField(max_length=20, default='dog')
